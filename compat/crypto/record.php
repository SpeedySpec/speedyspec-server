<?php
/**
 * Records WordPress's sodium functions into the fixtures the Rust crypto port is tested against.
 *
 * Usage: php record.php <wordpress-root> <out-dir>
 *
 * Run it with the static PHP the runtime embeds (`.php-build/buildroot/bin/php`), whose ext-sodium is the pinned
 * libsodium. Signing keys come from fixed seeds and Ed25519 is deterministic, so a re-record only changes when
 * WordPress or PHP changes. Error data is recorded without `php` and `sodium`, which a replay fills in from the
 * runtime it runs on; warnings PHP raised are recorded beside each case.
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
error_reporting(E_ALL & ~E_DEPRECATED);

require ABSPATH . WPINC . '/version.php';
require ABSPATH . WPINC . '/plugin.php';
// esc_html() asks for the blog charset; there is no database here.
add_filter('pre_option_blog_charset', static fn () => 'UTF-8');
require ABSPATH . WPINC . '/load.php';
require ABSPATH . WPINC . '/compat-utf8.php';
require ABSPATH . WPINC . '/utf8.php';
require ABSPATH . WPINC . '/compat.php';
require ABSPATH . WPINC . '/class-wp-error.php';
require ABSPATH . WPINC . '/functions.php';
require ABSPATH . WPINC . '/formatting.php';
require ABSPATH . WPINC . '/kses.php';
require ABSPATH . WPINC . '/pomo/translations.php';
require ABSPATH . WPINC . '/l10n.php';
require ABSPATH . 'wp-admin/includes/file.php';

$files = __DIR__ . '/files/';

function write_json($path, $value)
{
    $json = json_encode($value, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_THROW_ON_ERROR);
    file_put_contents($path, $json . "\n");
}

/** The result as JSON: true, false, or a WP_Error's code, message and data. */
function result($value)
{
    if ($value instanceof WP_Error) {
        $data = $value->get_error_data();
        if (is_array($data)) {
            unset($data['php'], $data['sodium']);
        }
        return array('code' => $value->get_error_code(), 'message' => $value->get_error_message(), 'data' => $data);
    }
    return $value;
}

/** Calls $fn and collects the warnings PHP raises meanwhile. */
function with_warnings(callable $fn, &$warnings)
{
    $warnings = array();
    set_error_handler(static function ($errno, $errstr) use (&$warnings) {
        $warnings[] = $errstr;
        return true;
    });
    try {
        return $fn();
    } finally {
        restore_error_handler();
    }
}

// Fast hashes.
$messages = array('', 'a', "\0\xff\xfe", "caf\xe9", 'An application password: ' . str_repeat('x', 24));
mt_srand(20261121);
$long = '';
for ($i = 0; $i < 4096; $i++) {
    $long .= chr(mt_rand(0, 255));
}
$messages[] = $long;

$fast_hash = array();
$verify    = array();
foreach ($messages as $message) {
    $hash        = wp_fast_hash($message);
    $fast_hash[] = array('message_b64' => base64_encode($message), 'hash' => $hash);
    $verify[]    = array('name' => 'own-hash', 'message_b64' => base64_encode($message), 'hash' => $hash, 'result' => wp_verify_fast_hash($message, $hash));
    $verify[]    = array('name' => 'partial', 'message_b64' => base64_encode($message), 'hash' => substr($hash, 0, -1), 'result' => wp_verify_fast_hash($message, substr($hash, 0, -1)));
    $verify[]    = array('name' => 'unprefixed', 'message_b64' => base64_encode($message), 'hash' => substr($hash, 9), 'result' => wp_verify_fast_hash($message, substr($hash, 9)));
}
// phpass, which WordPress still verifies and SpeedySpec does not: the replay expects false here.
require_once ABSPATH . WPINC . '/class-phpass.php';
$phpass   = (new PasswordHash(8, true))->HashPassword('password');
$verify[] = array('name' => 'phpass', 'message_b64' => base64_encode('password'), 'hash' => $phpass, 'result' => wp_verify_fast_hash('password', $phpass));

// Signatures. Keys from fixed seeds; signatures over the files' sha384, as WordPress.org signs packages.
$pair1 = sodium_crypto_sign_seed_keypair(str_repeat("\x01", 32));
$pair2 = sodium_crypto_sign_seed_keypair(str_repeat("\x02", 32));
$key1  = base64_encode(sodium_crypto_sign_publickey($pair1));
$key2  = base64_encode(sodium_crypto_sign_publickey($pair2));
$sign  = static function ($pair, $message) {
    return base64_encode(sodium_crypto_sign_detached($message, sodium_crypto_sign_secretkey($pair)));
};
$hello   = hash_file('sha384', $files . 'hello.txt', true);
$binary  = hash_file('sha384', $files . 'binary.bin', true);
$sig1    = $sign($pair1, $hello);
$sig2    = $sign($pair2, $hello);
$sig_bin = $sign($pair1, $binary);
$sig_nil = $sign($pair1, '');

$cases = array(
    array('valid', 'hello.txt', $sig1, array($key1), 'WordPress'),
    array('valid-array', 'hello.txt', array($sig1), array($key1), 'WordPress'),
    array('wrong-key', 'hello.txt', $sig1, array($key2), 'WordPress'),
    array('wrong-file', 'binary.bin', $sig1, array($key1), 'WordPress'),
    array('binary', 'binary.bin', $sig_bin, array($key1), 'WordPress'),
    array('short-signature', 'hello.txt', substr($sig1, 0, 84), array($key1), 'WordPress'),
    array('short-key', 'hello.txt', $sig1, array(substr($key1, 0, 40)), 'WordPress'),
    array('bad-base64', 'hello.txt', '!!!not base64!!!', array($key1), 'WordPress'),
    array('signature-with-newlines', 'hello.txt', chunk_split($sig1, 16, "\n"), array($key1), 'WordPress'),
    array('two-signatures-second-valid', 'hello.txt', array($sig2, $sig1), array($key1), 'WordPress'),
    array('two-keys-second-valid', 'hello.txt', $sig1, array($key2, $key1), 'WordPress'),
    array('skips-counted', 'hello.txt', array('QUJD', $sig2), array('QUJD', $key1), 'WordPress'),
    array('signatures-zero-string', 'hello.txt', '0', array($key1), 'WordPress'),
    array('signatures-empty-string', 'hello.txt', '', array($key1), 'WordPress'),
    array('signatures-empty-array', 'hello.txt', array(), array($key1), 'WordPress'),
    array('signatures-array-of-empty', 'hello.txt', array(''), array($key1), 'WordPress'),
    array('no-keys', 'hello.txt', $sig1, array(), 'WordPress'),
    array('filter-string', 'hello.txt', $sig1, $key1, 'WordPress'),
    array('filter-null', 'hello.txt', $sig1, null, 'WordPress'),
    array('filter-int-key', 'hello.txt', $sig1, array(5, $key1), 'WordPress'),
    array('filter-assoc', 'hello.txt', $sig1, array('primary' => $key2, 'backup' => $key1), 'WordPress'),
    array('filter-array-key-after-match', 'hello.txt', $sig1, array($key1, array('x')), 'WordPress'),
    array('filter-array-key-short-signature', 'hello.txt', 'QUJD', array(array('x')), 'WordPress'),
    array('array-signature-after-match', 'hello.txt', array($sig1, array('x')), array($key1), 'WordPress'),
    array('empty-file', 'empty.txt', $sig1, array($key1), 'WordPress'),
    array('missing-file', 'missing.zip', $sig1, array($key1), 'WordPress'),
    array('missing-file-signed-empty', 'missing.zip', $sig_nil, array($key1), 'WordPress'),
    array('default-filename', 'hello.txt', $sig2, array($key1), false),
    array('escaped-filename', 'hello.txt', $sig2, array($key1), 'a<b>&"c\'.zip'),
);

$signature = array();
foreach ($cases as [$name, $file, $signatures, $keys, $filename_for_errors]) {
    remove_all_filters('wp_trusted_keys');
    add_filter('wp_trusted_keys', static function () use ($keys) {
        return $keys;
    });
    $value = with_warnings(static function () use ($files, $file, $signatures, $filename_for_errors) {
        return verify_file_signature($files . $file, $signatures, $filename_for_errors);
    }, $warnings);
    $signature[] = array(
        'name'                => $name,
        'file'                => $file,
        'signatures'          => $signatures,
        'filter'              => $keys,
        'filename_for_errors' => $filename_for_errors,
        'result'              => result($value),
        'warnings'            => str_replace($files, '<files>/', $warnings),
    );
}
remove_all_filters('wp_trusted_keys');
$defaults = wp_trusted_keys();

write_json(rtrim($out, '/') . '/cases.json', array(
    'wordpress'    => $wp_version,
    'php'          => PHP_VERSION,
    'sodium'       => SODIUM_LIBRARY_VERSION,
    'trusted_keys' => $defaults,
    'fast_hash'    => $fast_hash,
    'verify'       => $verify,
    'signature'    => $signature,
));
