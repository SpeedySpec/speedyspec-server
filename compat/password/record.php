<?php
/**
 * Records WordPress's password functions into the fixtures the Rust password port is tested against.
 *
 * Usage: php record.php <wordpress-root> <out-dir>
 *
 * Run it with the static PHP the runtime embeds (`.php-build/buildroot/bin/php`). Hashing is salted, so random
 * outputs are recorded as their shape (everything up to the last `$`) and whether they verify; the deterministic
 * pieces (the pre-hash, bcrypt through `crypt()` with a given salt, raw argon2 through `sodium_crypto_pwhash()`) are
 * recorded whole. Passwords are hex so binary ones survive JSON (a long run of one byte is `{repeat, times}`). Warnings PHP raised are recorded beside each case.
 */

declare(strict_types=1);

[$_, $root, $out] = $argv + [null, null, null];
if (!$root || !$out) {
    fwrite(STDERR, "usage: php record.php <wordpress-root> <out-dir>\n");
    exit(2);
}
$root = rtrim($root, '/') . '/';
define('ABSPATH', $root);
define('WPINC', 'wp-includes');
error_reporting(E_ALL);

require ABSPATH . WPINC . '/version.php';
require ABSPATH . WPINC . '/plugin.php';
require ABSPATH . WPINC . '/load.php';
require ABSPATH . WPINC . '/compat-utf8.php';
require ABSPATH . WPINC . '/compat.php';
require ABSPATH . WPINC . '/functions.php';
require ABSPATH . WPINC . '/pluggable.php';
require ABSPATH . WPINC . '/class-phpass.php';
require ABSPATH . WPINC . '/class-wp-application-passwords.php';

/** Warnings raised by the current case. */
$warnings = [];
set_error_handler(static function (int $no, string $message) use (&$warnings, $root): bool {
    $warnings[] = str_replace($root, '<files>/', $message);
    return true;
});

/**
 * Runs `$fn`, recording its result, exception and warnings.
 */
function run(callable $fn): array
{
    global $warnings;
    $warnings = [];
    $case = [];
    try {
        $case['result'] = $fn();
    } catch (Throwable $e) {
        $case['exception'] = ['class' => get_class($e), 'message' => $e->getMessage()];
    }
    $case['warnings'] = $warnings;
    return $case;
}

/**
 * A password for JSON: its hex, or `{repeat, times}` for a long run of one byte.
 */
function pw(string $p): string|array
{
    if (strlen($p) > 100 && $p === str_repeat($p[0], strlen($p))) {
        return ['repeat' => bin2hex($p[0]), 'times' => strlen($p)];
    }
    return bin2hex($p);
}

/** A hash's shape: everything up to its last `$`. */
function head(string $hash): string
{
    return preg_replace('/[^$]*$/', '', $hash);
}

/** Replaces the algorithm and options filters for the duration of `$fn`. */
function with_filters($algorithm, $options, callable $fn)
{
    remove_all_filters('wp_hash_password_algorithm');
    remove_all_filters('wp_hash_password_options');
    if ($algorithm !== '__default__') {
        add_filter('wp_hash_password_algorithm', static fn () => $algorithm);
    }
    if ($options !== '__default__') {
        add_filter('wp_hash_password_options', static fn () => $options);
    }
    try {
        return $fn();
    } finally {
        remove_all_filters('wp_hash_password_algorithm');
        remove_all_filters('wp_hash_password_options');
    }
}

$passwords = [
    'empty'      => '',
    'plain'      => 'password',
    'spaced'     => ' pass ',
    'tab-nl'     => "\tpass\n",
    'vtab-nul'   => "\x0Bpass\0",
    'unicode'    => 'pässwörd',
    'binary'     => "\xff\xfepw",
    'nul-inside' => "pw\0x",
    'len-72'     => str_repeat('a', 72),
    'len-73'     => str_repeat('a', 73),
    'len-4096'   => str_repeat('a', 4096),
    'len-4097'   => str_repeat('a', 4097),
];

$salts22 = ['abcdefghijklmnopqrstuu', 'ABCDEFGHIJKLMNOPQRSTUu', '......................'];
$salts16 = [str_repeat("\x01", 16), '0123456789abcdef'];

$cases = [];

// The pre-hash, untrimmed (the function, not WordPress's trim before it).
foreach ($passwords as $name => $p) {
    $cases['prehash'][] = [
        'name'         => "prehash/$name",
        'password' => pw($p),
        'result'       => base64_encode(hash_hmac('sha384', $p, 'wp-sha384', true)),
    ];
}

// bcrypt with a given salt, as crypt() computes it.
foreach (['$2y$', '$2a$', '$2b$', '$2x$'] as $prefix) {
    foreach ([4, 5, 10] as $cost) {
        foreach ($salts22 as $i => $salt) {
            foreach (['plain', 'unicode', 'binary', 'len-72', 'len-73', 'empty', 'nul-inside'] as $name) {
                $p = $passwords[$name];
                $setting = $prefix . sprintf('%02d', $cost) . '$' . $salt;
                $case = run(static fn () => crypt($p, $setting));
                $hash = $case['result'] ?? '';
                $cases['bcrypt'][] = [
                    'name'         => 'bcrypt/' . trim($prefix, '$') . "/$cost/$i/$name",
                    'password' => pw($p),
                    'setting'      => $setting,
                    'verify_right' => run(static fn () => password_verify($p, $hash)),
                    'verify_wrong' => run(static fn () => password_verify($p . 'x', $hash)),
                ] + $case;
            }
        }
    }
}

// Raw argon2, as sodium_crypto_pwhash() computes it.
$variants = ['argon2i' => SODIUM_CRYPTO_PWHASH_ALG_ARGON2I13, 'argon2id' => SODIUM_CRYPTO_PWHASH_ALG_ARGON2ID13];
foreach ($variants as $variant => $alg) {
    foreach ([[4, 65536], [3, 4096], [1, 8], [2, 4096]] as [$t, $m]) {
        foreach ($salts16 as $i => $salt) {
            foreach (['plain', 'binary', 'empty'] as $name) {
                $p = $passwords[$name];
                $case = run(static fn () => bin2hex(sodium_crypto_pwhash(32, $p, $salt, $t, $m * 1024, $alg)));
                $cases['argon2_raw'][] = [
                    'name'         => "argon2_raw/$variant/$t/$m/$i/$name",
                    'variant'      => $variant,
                    'password' => pw($p),
                    'salt_hex'     => bin2hex($salt),
                    'time_cost'    => $t,
                    'memory_kib'   => $m,
                ] + $case;
            }
        }
    }
}

// argon2 hashes as password_hash() writes them, and whether they verify.
foreach (['argon2i' => PASSWORD_ARGON2I, 'argon2id' => PASSWORD_ARGON2ID] as $variant => $algo) {
    foreach ([[], ['time_cost' => 3, 'memory_cost' => 4096]] as $j => $options) {
        foreach (['plain', 'binary', 'spaced'] as $name) {
            $p = $passwords[$name];
            $hash = password_hash($p, $algo, $options);
            $cases['argon2_php'][] = [
                'name'         => "argon2_php/$variant/$j/$name",
                'password' => pw($p),
                'hash'         => $hash,
                'verify_right' => password_verify($p, $hash),
                'verify_wrong' => password_verify($p . 'x', $hash),
            ];
        }
    }
}

// wp_hash_password() under algorithm filters.
$algorithms = [
    'default'  => '__default__',
    'argon2id' => PASSWORD_ARGON2ID,
    'argon2i'  => PASSWORD_ARGON2I,
    'bcrypt'   => PASSWORD_BCRYPT,
    'null'     => null,
    'int-0'    => 0,
    'int-1'    => 1,
    'int-2'    => 2,
    'int-3'    => 3,
    'int-4'    => 4,
    'unknown'  => 'nope',
    'empty'    => '',
    'array'    => [],
    'true'     => true,
    'float-1'  => 1.0,
];
foreach ($algorithms as $aname => $algorithm) {
    foreach (['plain', 'spaced', 'nul-inside', 'len-73', 'len-4096', 'len-4097'] as $name) {
        $p = $passwords[$name];
        $case = run(static fn () => with_filters($algorithm, '__default__', static fn () => wp_hash_password($p)));
        if (isset($case['result'])) {
            $hash = $case['result'];
            $case['head'] = head($hash);
            $case['length'] = strlen($hash);
            $case['wp_check_password'] = run(static fn () => wp_check_password($p, $hash));
            $case['wp_check_password_trimmed'] = run(static fn () => wp_check_password(trim($p), $hash));
            unset($case['result']);
        }
        $cases['hash'][] = [
            'name'         => "hash/$aname/$name",
            'algorithm'    => $algorithm === '__default__' ? ['default' => true] : ['value' => $algorithm],
            'password' => pw($p),
        ] + $case;
    }
}

// Options through the filter.
$option_sets = [
    'bcrypt/cost-3'         => [PASSWORD_BCRYPT, ['cost' => 3]],
    'bcrypt/cost-4'         => [PASSWORD_BCRYPT, ['cost' => 4]],
    'bcrypt/cost-32'        => [PASSWORD_BCRYPT, ['cost' => 32]],
    'bcrypt/cost-string'    => [PASSWORD_BCRYPT, ['cost' => '5']],
    'bcrypt/cost-float'     => [PASSWORD_BCRYPT, ['cost' => 5.7]],
    'bcrypt/cost-null'      => [PASSWORD_BCRYPT, ['cost' => null]],
    'bcrypt/cost-bad'       => [PASSWORD_BCRYPT, ['cost' => 'abc']],
    'bcrypt/salt'           => [PASSWORD_BCRYPT, ['salt' => str_repeat('a', 22)]],
    'bcrypt/unknown-key'    => [PASSWORD_BCRYPT, ['nope' => 1]],
    'bcrypt/not-array-str'  => [PASSWORD_BCRYPT, 'x'],
    'bcrypt/not-array-null' => [PASSWORD_BCRYPT, null],
    'plain-bcrypt/cost-5'   => [1, ['cost' => 5]],
    'argon2id/m-7'          => [PASSWORD_ARGON2ID, ['memory_cost' => 7]],
    'argon2id/m-8'          => [PASSWORD_ARGON2ID, ['memory_cost' => 8]],
    'argon2id/m-4096'       => [PASSWORD_ARGON2ID, ['memory_cost' => 4096]],
    'argon2id/t-0'          => [PASSWORD_ARGON2ID, ['time_cost' => 0]],
    'argon2id/t-1'          => [PASSWORD_ARGON2ID, ['time_cost' => 1, 'memory_cost' => 4096]],
    'argon2id/t-2'          => [PASSWORD_ARGON2ID, ['time_cost' => 2, 'memory_cost' => 4096]],
    'argon2id/threads-1'    => [PASSWORD_ARGON2ID, ['threads' => 1, 'memory_cost' => 4096]],
    'argon2id/threads-2'    => [PASSWORD_ARGON2ID, ['threads' => 2]],
    'argon2id/m-string'     => [PASSWORD_ARGON2ID, ['memory_cost' => '4096']],
    'argon2id/salt'         => [PASSWORD_ARGON2ID, ['salt' => str_repeat('a', 16), 'memory_cost' => 4096]],
    'argon2id/not-array'    => [PASSWORD_ARGON2ID, 'x'],
    'argon2i/t-1'           => [PASSWORD_ARGON2I, ['time_cost' => 1]],
    'argon2i/t-2'           => [PASSWORD_ARGON2I, ['time_cost' => 2]],
    'argon2i/t-3'           => [PASSWORD_ARGON2I, ['time_cost' => 3, 'memory_cost' => 4096]],
];
foreach ($option_sets as $oname => [$algorithm, $options]) {
    $p = 'password';
    $case = run(static fn () => with_filters($algorithm, $options, static fn () => wp_hash_password($p)));
    if (isset($case['result'])) {
        $hash = $case['result'];
        $case['head'] = head($hash);
        $case['length'] = strlen($hash);
        $case['wp_check_password'] = wp_check_password($p, $hash);
        unset($case['result']);
    }
    $cases['options'][] = [
        'name'      => "options/$oname",
        'algorithm' => ['value' => $algorithm],
        'options'   => $options,
    ] + $case;
}

// wp_check_password() over every format.
$phpass = new PasswordHash(8, true);
$P = $phpass->HashPassword('password');
$formats = [
    'wp-2y'        => with_filters('__default__', '__default__', static fn () => wp_hash_password('password')),
    'wp-2y-spaced' => with_filters('__default__', '__default__', static fn () => wp_hash_password(' password ')),
    '2y'           => password_hash('password', PASSWORD_BCRYPT, ['cost' => 5]),
    '2a'           => crypt('password', '$2a$05$' . $salts22[0]),
    '2b'           => crypt('password', '$2b$05$' . $salts22[0]),
    '2x'           => crypt('password', '$2x$05$' . $salts22[0]),
    'argon2i'      => password_hash('password', PASSWORD_ARGON2I, ['time_cost' => 3, 'memory_cost' => 4096]),
    'argon2id'     => password_hash('password', PASSWORD_ARGON2ID, ['time_cost' => 1, 'memory_cost' => 4096]),
    'wp-argon2id'  => '$wp' . password_hash(base64_encode(hash_hmac('sha384', 'password', 'wp-sha384', true)), PASSWORD_ARGON2ID, ['time_cost' => 1, 'memory_cost' => 4096]),
    'phpass'       => $P,
    'phpass-H'     => '$H$' . substr($P, 3),
    'md5'          => md5('password'),
    'md5-crypt'    => crypt('password', '$1$saltsalt$'),
    'sha256-crypt' => crypt('password', '$5$rounds=5000$saltsaltsaltsalt$'),
    'sha512-crypt' => crypt('password', '$6$rounds=5000$saltsaltsaltsalt$'),
    'des'          => crypt('password', 'ab'),
    'len-32'       => str_repeat('x', 32),
    'len-33'       => str_repeat('x', 33),
    'empty'        => '',
    'wp-only'      => '$wp',
    'garbage'      => '$2y$05$' . str_repeat('!', 53),
];
$attempts = ['right' => 'password', 'wrong' => 'passwordx', 'spaced' => ' password ', 'empty' => '', 'len-4097' => str_repeat('a', 4097)];
foreach ($formats as $fname => $hash) {
    foreach ($attempts as $aname => $p) {
        $case = run(static fn () => wp_check_password($p, $hash));
        $cases['check'][] = [
            'name'         => "check/$fname/$aname",
            'format'       => $fname,
            'password' => pw($p),
            'hash'         => $hash,
        ] + $case;
    }
}

// password_needs_rehash() and wp_password_needs_rehash().
$settings = [
    'bcrypt-5'         => [PASSWORD_BCRYPT, ['cost' => 5]],
    'bcrypt-default'   => [PASSWORD_BCRYPT, []],
    'argon2id-default' => [PASSWORD_ARGON2ID, []],
    'argon2id-1-4096'  => [PASSWORD_ARGON2ID, ['time_cost' => 1, 'memory_cost' => 4096]],
    'argon2i-3-4096'   => [PASSWORD_ARGON2I, ['time_cost' => 3, 'memory_cost' => 4096]],
    'plain-bcrypt-1'   => [1, ['cost' => 5]],
    'null'             => [null, []],
    'unknown'          => ['nope', []],
    'int-4'            => [4, []],
    'array-algo'       => [[], []],
    'options-string'   => [PASSWORD_BCRYPT, 'x'],
    'argon2-options-string' => [PASSWORD_ARGON2ID, 'x'],
    'bcrypt-cost-string' => [PASSWORD_BCRYPT, ['cost' => '5']],
];
foreach ($formats as $fname => $hash) {
    foreach ($settings as $sname => [$algorithm, $options]) {
        $unprefixed = str_starts_with($hash, '$wp') ? substr($hash, 3) : $hash;
        $php = run(static fn () => password_needs_rehash($unprefixed, $algorithm, $options));
        $wp = run(static fn () => with_filters($algorithm, $options, static fn () => wp_password_needs_rehash($hash)));
        $cases['needs_rehash'][] = [
            'name'      => "needs_rehash/$fname/$sname",
            'hash'      => $hash,
            'algorithm' => ['value' => $algorithm],
            'options'   => $options,
            'php'       => $php,
            'wp'        => $wp,
        ];
    }
}

// wp_rand()'s argument rules (its draws are random): the range observed over many draws.
foreach ([
    'defaults'  => [null, null],
    'zero-zero' => [0, 0],
    'five-five' => [5, 5],
    'reversed'  => [10, 1],
    'negative'  => [-5, -1],
    'strings'   => ['3', '7'],
    'floats'    => [1.9, 3.2],
    'min-only'  => [5, null],
    'small'     => [0, 1],
] as $rname => [$min, $max]) {
    $seen = [];
    for ($i = 0; $i < 400; $i++) {
        $seen[] = wp_rand($min, $max);
    }
    $cases['rand'][] = [
        'name'     => "rand/$rname",
        'min'      => $min,
        'max'      => $max,
        'lowest'   => min($seen),
        'highest'  => max($seen),
        'all_ints' => array_reduce($seen, static fn ($c, $v) => $c && is_int($v), true),
    ];
}

// wp_generate_password(): the characters it draws from, and the random_password filter's arguments.
foreach ([[true, false], [false, false], [true, true], [false, true]] as [$special, $extra]) {
    $cases['generate'][] = [
        'name'    => 'generate/' . ($special ? 'special' : 'plain') . ($extra ? '-extra' : ''),
        'special' => $special,
        'extra'   => $extra,
        'chars'   => count_chars(wp_generate_password(20000, $special, $extra), 3),
    ];
}
$filter_args = null;
add_filter('random_password', static function (...$args) use (&$filter_args) {
    $filter_args = array_slice($args, 1);
    return $args[0];
}, 10, 4);
foreach (['length-0' => 0, 'length-negative' => -3, 'length-string' => '5', 'length-float' => 2.5, 'length-7' => 7] as $gname => $length) {
    $filter_args = null;
    $case = run(static fn () => strlen(wp_generate_password($length, false, true)));
    $cases['generate_length'][] = ['name' => "generate_length/$gname", 'length' => $length, 'filter_args' => $filter_args] + $case;
}
remove_all_filters('random_password');

// Application passwords' chunking.
foreach ([0, 3, 4, 5, 24] as $length) {
    $raw = substr('abcdefghijklmnopqrstuvwxyz', 0, $length);
    $cases['chunk'][] = [
        'name'   => "chunk/$length",
        'raw'    => $raw,
        'result' => WP_Application_Passwords::chunk_password($raw),
    ];
}

$cases['meta'] = [
    'wordpress' => $GLOBALS['wp_version'],
    'php'       => PHP_VERSION,
    'sodium'    => SODIUM_LIBRARY_VERSION,
    'algos'     => password_algos(),
];

file_put_contents(
    rtrim($out, '/') . '/cases.json',
    json_encode($cases, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_INVALID_UTF8_SUBSTITUTE | JSON_THROW_ON_ERROR) . "\n"
);
