<?php
/**
 * Records WordPress's meta key rules into the fixtures the Rust meta port is tested against.
 *
 * Usage: php record.php <wordpress-root> <out-dir>
 *
 * Run it with the static PHP the runtime embeds (`.php-build/buildroot/bin/php`) against the WordPress 7.1.5 root
 * that `speedyspec_compat::wordpress_release("7.1.5")` unpacks. It records three things into `<out-dir>/cases.json`:
 *
 * - `protected`: `is_protected_meta()` over keys with control, non-ASCII and Latin-1 bytes, under meta types
 *   `post` and `""`.
 * - `register`: named scenarios of `register_meta()`, `unregister_meta_key()`, `registered_meta_key_exists()` and
 *   `get_registered_meta_keys()` calls. `$wp_meta_keys` and `$wp_filter` are reset at the start of each scenario;
 *   within one, state carries over from call to call. Each call records its arguments, its result, the
 *   `_doing_it_wrong()` messages it raised (captured through the `doing_it_wrong_run` action) and the filters it
 *   added and removed (a diff of `$wp_filter` before and after). Each scenario ends with the whole `$wp_meta_keys`.
 * - `default`: `filter_default_metadata()` called directly after a scenario's `register_meta()` calls. The value
 *   passed in is the one `get_metadata_default()` passes: `''` when single, `array()` otherwise. `subtype` is what
 *   `get_object_subtype()` answers for the object under the stubs below; it is recorded for `post` only and is null
 *   for other types, since no comment, term or user lookup is stubbed and `filter_default_metadata()` reaches one
 *   only when it finds a default under a non-empty subtype.
 *
 * Value encoding. Every PHP value is written as `{"t": <type>, "v": <value>}`, so key order and bytes survive JSON:
 *
 * - `{"t":"null"}` (no `v`), `{"t":"bool","v":true}`, `{"t":"int","v":10}`, `{"t":"float","v":1.5}`;
 * - `{"t":"str","v":"<lowercase hex of the bytes>"}`;
 * - `{"t":"array","v":[[key, value], ...]}` in PHP's order, where a key is `{"t":"int","v":N}` or
 *   `{"t":"str","v":"<hex>"}` and a value is encoded the same way, recursively.
 *
 * Plain JSON fields: meta keys as `key_hex` (lowercase hex), meta types and subtypes as plain strings, object ids
 * as ints, `single` as a bool. In `filters` and `filters_removed` each entry is `[hook, callback, priority,
 * accepted_args]`: hook and callback are encoded values (callbacks are named functions, so strings), priority and
 * accepted_args plain ints. `doing_it_wrong` entries are `{function, message, version}` in plain text, the message
 * being the one `__()` returns.
 *
 * Loaded from WordPress: version.php, plugin.php, load.php (wp_installing, is_wp_error), class-wp-error.php,
 * functions.php (wp_parse_args, _doing_it_wrong, __return_true/false; it requires option.php), formatting.php,
 * meta.php. default-filters.php is not loaded, so `_wp_register_meta_args_allowed_list` is not hooked except in the
 * scenario that hooks it by name.
 *
 * Stubbed, because meta.php calls them and they live in files not loaded here:
 * - `__()` (l10n.php) returns its text;
 * - `post_type_supports()` (post.php) is true only for `post` and `page`;
 * - `get_post_type()` (post.php) is `'page'` for object 10, `'post'` for 11, false otherwise;
 * - `rest_validate_value_from_schema()` (rest-api.php) returns a WP_Error only when the value's PHP type differs from
 *   `$schema['type']` (string, integer, number, boolean, array, object), true otherwise.
 * `_doing_it_wrong()` is WordPress's own (functions.php), observed through `doing_it_wrong_run`, with WP_DEBUG off.
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
define('WP_DEBUG', false);
error_reporting(E_ALL);

/** Warnings PHP raised; the run fails if any are left. */
$warnings = [];
set_error_handler(static function (int $no, string $message) use (&$warnings, $root): bool {
    $warnings[] = str_replace($root, '<files>/', $message);
    return true;
});

// Stubs for what meta.php calls outside the files loaded below.
function __($text, $domain = 'default')
{
    return $text;
}

function post_type_supports($post_type, $feature)
{
    return in_array($post_type, ['post', 'page'], true);
}

function get_post_type($post = null)
{
    if (10 === $post) {
        return 'page';
    }
    if (11 === $post) {
        return 'post';
    }
    return false;
}

function rest_validate_value_from_schema($value, $args, $param = '')
{
    $type = $args['type'] ?? null;
    $ok = match ($type) {
        'string'  => is_string($value),
        'integer' => is_int($value),
        'number'  => is_int($value) || is_float($value),
        'boolean' => is_bool($value),
        'array', 'object' => is_array($value),
        default   => true,
    };
    return $ok ? true : new WP_Error('rest_invalid_type', 'invalid type');
}

require ABSPATH . WPINC . '/version.php';
require ABSPATH . WPINC . '/plugin.php';
require ABSPATH . WPINC . '/load.php';
require ABSPATH . WPINC . '/class-wp-error.php';
require ABSPATH . WPINC . '/functions.php';
require ABSPATH . WPINC . '/formatting.php';
require ABSPATH . WPINC . '/meta.php';

/** A named sanitize callback. */
function speedyspec_sanitize_meta($value)
{
    return $value;
}

/** A named auth callback. */
function speedyspec_auth_meta()
{
    return true;
}

/** Encodes a PHP value as `{"t", "v"}`. */
function enc($value): array
{
    return match (true) {
        null === $value     => ['t' => 'null'],
        is_bool($value)     => ['t' => 'bool', 'v' => $value],
        is_int($value)      => ['t' => 'int', 'v' => $value],
        is_float($value)    => ['t' => 'float', 'v' => $value],
        is_string($value)   => ['t' => 'str', 'v' => bin2hex($value)],
        is_array($value)    => ['t' => 'array', 'v' => array_map(
            static fn ($k, $v) => [enc($k), enc($v)],
            array_keys($value),
            array_values($value)
        )],
        default => throw new RuntimeException('cannot encode ' . get_debug_type($value)),
    };
}

/** The `_doing_it_wrong()` calls of the current call. */
$doing_it_wrong = [];

function record_doing_it_wrong($function_name, $message, $version): void
{
    $GLOBALS['doing_it_wrong'][] = ['function' => $function_name, 'message' => $message, 'version' => $version];
}

/** Empties `$wp_filter` and `$wp_meta_keys`, keeping only the recorder's own action. */
function reset_state(): void
{
    $GLOBALS['wp_filter'] = [];
    $GLOBALS['wp_meta_keys'] = null;
    add_action('doing_it_wrong_run', 'record_doing_it_wrong', 10, 3);
}

/** Every hooked callback as `"hook|priority|id" => [hook, callback, priority, accepted_args]`. */
function filter_snapshot(): array
{
    $snapshot = [];
    foreach ($GLOBALS['wp_filter'] as $hook => $wp_hook) {
        if ('doing_it_wrong_run' === $hook) {
            continue;
        }
        foreach ($wp_hook->callbacks as $priority => $callbacks) {
            foreach ($callbacks as $id => $callback) {
                $snapshot["$hook|$priority|$id"] = [
                    enc((string) $hook),
                    enc($callback['function']),
                    (int) $priority,
                    (int) $callback['accepted_args'],
                ];
            }
        }
    }
    return $snapshot;
}

/** Calls `$fn(...$args)`, recording its result, `_doing_it_wrong()` messages and filter changes. */
function call(string $fn, array $args): array
{
    global $doing_it_wrong, $warnings;
    $doing_it_wrong = [];
    $before = filter_snapshot();
    $result = $fn(...$args);
    $after = filter_snapshot();
    if ($warnings) {
        throw new RuntimeException("$fn raised: " . implode('; ', $warnings));
    }
    return [
        'fn'              => $fn,
        'args'            => enc($args),
        'result'          => enc($result),
        'doing_it_wrong'  => $doing_it_wrong,
        'filters'         => array_values(array_diff_key($after, $before)),
        'filters_removed' => array_values(array_diff_key($before, $after)),
    ];
}

/** Runs a scenario's calls from a clean state. */
function scenario(string $name, array $calls): array
{
    reset_state();
    $recorded = [];
    foreach ($calls as [$fn, $args]) {
        $recorded[] = call($fn, $args);
    }
    return ['name' => $name, 'calls' => $recorded, 'wp_meta_keys' => enc($GLOBALS['wp_meta_keys'])];
}

$cases = ['wordpress' => $GLOBALS['wp_version'], 'php' => PHP_VERSION, 'protected' => [], 'register' => [], 'default' => []];
if ('7.1.5' !== $cases['wordpress']) {
    fwrite(STDERR, "expected WordPress 7.1.5, found {$cases['wordpress']}\n");
    exit(1);
}

// is_protected_meta().
$keys = ['', '_', '_x', 'x_', ' _x', "\x00_x", "\x7F_x", "\xC2\xA0_x", "\xAA_x", "\xC0_x", "\xD7_x", 'é_x', "\t_x", '__proto'];
reset_state();
foreach ($keys as $key) {
    foreach (['post', ''] as $meta_type) {
        $cases['protected'][] = [
            'key_hex'   => bin2hex($key),
            'meta_type' => $meta_type,
            'result'    => enc(is_protected_meta($key, $meta_type)),
        ];
    }
}

// register_meta() and friends.
$register = [
    'defaults-only' => [
        ['register_meta', ['post', 'color', []]],
    ],
    'single-default' => [
        ['register_meta', ['post', 'color', ['single' => true, 'default' => 'blue']]],
    ],
    'subtype-page' => [
        ['register_meta', ['post', 'color', ['object_subtype' => 'page', 'single' => true]]],
    ],
    'unknown-arg-first' => [
        ['register_meta', ['post', 'color', ['foo' => 'bar', 'type' => 'integer', 'single' => true]]],
    ],
    'unknown-arg-last' => [
        ['register_meta', ['post', 'color', ['type' => 'integer', 'single' => true, 'foo' => 'bar']]],
    ],
    'unknown-arg-allowed-list' => [
        ['add_filter', ['register_meta_args', '_wp_register_meta_args_allowed_list', 10, 2]],
        ['register_meta', ['post', 'color', ['foo' => 'bar', 'type' => 'integer', 'default' => 3, 'single' => true]]],
    ],
    'array-show-in-rest-true' => [
        ['register_meta', ['post', 'tags', ['type' => 'array', 'show_in_rest' => true]]],
    ],
    'array-show-in-rest-items' => [
        ['register_meta', ['post', 'tags', ['type' => 'array', 'show_in_rest' => ['schema' => ['items' => ['type' => 'string']]]]]],
    ],
    'revisions-comment' => [
        ['register_meta', ['comment', 'mood', ['revisions_enabled' => true]]],
    ],
    'revisions-attachment' => [
        ['register_meta', ['post', 'mood', ['object_subtype' => 'attachment', 'revisions_enabled' => true]]],
    ],
    'revisions-page' => [
        ['register_meta', ['post', 'mood', ['object_subtype' => 'page', 'revisions_enabled' => true]]],
    ],
    'default-mismatch' => [
        ['register_meta', ['post', 'count', ['type' => 'integer', 'default' => 'x']]],
    ],
    'auth-protected' => [
        ['register_meta', ['post', '_secret', []]],
    ],
    'auth-unprotected' => [
        ['register_meta', ['post', 'public', []]],
    ],
    'auth-named' => [
        ['register_meta', ['post', '_secret', ['auth_callback' => 'speedyspec_auth_meta']]],
    ],
    'sanitize-named' => [
        ['register_meta', ['post', 'color', ['sanitize_callback' => 'speedyspec_sanitize_meta']]],
    ],
    'sanitize-named-subtype' => [
        ['register_meta', ['post', 'color', ['object_subtype' => 'page', 'sanitize_callback' => 'speedyspec_sanitize_meta']]],
    ],
    'legacy-args-callable' => [
        ['register_meta', ['post', 'color', 'speedyspec_sanitize_meta']],
    ],
    'legacy-deprecated-callable' => [
        ['register_meta', ['post', 'color', [], 'speedyspec_auth_meta']],
    ],
    'unregister-clean-up' => [
        ['registered_meta_key_exists', ['post', 'color']],
        ['get_registered_meta_keys', ['post']],
        ['register_meta', ['post', 'color', ['sanitize_callback' => 'speedyspec_sanitize_meta']]],
        ['register_meta', ['post', 'size', ['object_subtype' => 'page', 'default' => 'm', 'single' => true]]],
        ['registered_meta_key_exists', ['post', 'color']],
        ['registered_meta_key_exists', ['post', 'size']],
        ['registered_meta_key_exists', ['post', 'size', 'page']],
        ['get_registered_meta_keys', ['post']],
        ['get_registered_meta_keys', ['post', 'page']],
        ['unregister_meta_key', ['post', 'color']],
        ['registered_meta_key_exists', ['post', 'color']],
        ['get_registered_meta_keys', ['post']],
        ['unregister_meta_key', ['post', 'size', 'page']],
        ['registered_meta_key_exists', ['post', 'size', 'page']],
        ['get_registered_meta_keys', ['post', 'page']],
        ['get_registered_meta_keys', ['post']],
    ],
    'unregister-missing' => [
        ['unregister_meta_key', ['post', 'missing']],
        ['register_meta', ['post', 'color', []]],
        ['unregister_meta_key', ['post', 'missing']],
        ['unregister_meta_key', ['post', 'color', 'page']],
        ['unregister_meta_key', ['user', 'color']],
    ],
];
foreach ($register as $name => $calls) {
    $cases['register'][] = scenario($name, $calls);
}

// filter_default_metadata().
$defaults = [
    'empty-subtype' => [
        [['register_meta', ['post', 'color', ['single' => true, 'default' => 'blue']]]],
        [['post', 10, 'color', true], ['post', 10, 'color', false], ['post', 99, 'color', true]],
    ],
    'page-subtype' => [
        [['register_meta', ['post', 'color', ['object_subtype' => 'page', 'default' => 'red']]]],
        [['post', 10, 'color', true], ['post', 10, 'color', false], ['post', 11, 'color', true], ['post', 11, 'color', false]],
    ],
    'empty-subtype-wins' => [
        [
            ['register_meta', ['post', 'color', ['object_subtype' => 'page', 'default' => 'red']]],
            ['register_meta', ['post', 'color', ['default' => 'blue']]],
        ],
        [['post', 10, 'color', true], ['post', 11, 'color', true]],
    ],
    'nothing-registered' => [
        [['register_meta', ['post', 'color', ['default' => 'blue']]]],
        [['comment', 10, 'color', true], ['comment', 10, 'color', false]],
    ],
    'no-default' => [
        [['register_meta', ['post', 'color', ['single' => true]]]],
        [['post', 10, 'color', true], ['post', 10, 'color', false]],
    ],
    'other-key' => [
        [['register_meta', ['post', 'color', ['default' => 'blue']]]],
        [['post', 10, 'size', true]],
    ],
];
foreach ($defaults as $name => [$calls, $lookups]) {
    $setup = scenario("default/$name", $calls);
    foreach ($lookups as [$meta_type, $object_id, $key, $single]) {
        $value = $single ? '' : [];
        $cases['default'][] = [
            'name'      => "$name/$meta_type/$object_id/" . ($single ? 'single' : 'multiple'),
            'register'  => $setup['calls'],
            'meta_type' => $meta_type,
            'object_id' => $object_id,
            'key_hex'   => bin2hex($key),
            'single'    => $single,
            'subtype'   => 'post' === $meta_type ? get_object_subtype($meta_type, $object_id) : null,
            'value'     => enc($value),
            'result'    => enc(filter_default_metadata($value, $object_id, $key, $single, $meta_type)),
        ];
    }
}

if ($warnings) {
    fwrite(STDERR, "warnings:\n" . implode("\n", $warnings) . "\n");
    exit(1);
}

file_put_contents(
    rtrim($out, '/') . '/cases.json',
    json_encode($cases, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR) . "\n"
);
printf(
    "WordPress %s, PHP %s: %d protected, %d register scenarios (%d calls), %d default cases\n",
    $cases['wordpress'],
    $cases['php'],
    count($cases['protected']),
    count($cases['register']),
    array_sum(array_map(static fn ($s) => count($s['calls']), $cases['register'])),
    count($cases['default'])
);
