<?php

declare(strict_types=1);

/**
 * Sets up the theme.json scenarios the same way on both sides.
 *
 *  `SPEEDYSPEC_COMPAT_WP_ROOT` is the release to load from and `SPEEDYSPEC_COMPAT_PORT` whether the native port is
 *  loaded. The recorder loads WordPress's `plugin.php` and its theme.json classes; the port's side loads neither
 *  (they are native). Both load the block registries and the formatting functions theme.json reaches, and the same
 *  stubs, so only the theme.json engine differs between the two runs.
 *
 *  Array results go through {@link compat_canon()}, so the fixture compares them as PHP's `==` does (the theme.json
 *  design's "same contents, schema order" rule). CSS strings are compared byte for byte.
 */

$GLOBALS['speedyspec_notices'] = [];

if (!defined('ABSPATH')) {
    define('ABSPATH', SPEEDYSPEC_COMPAT_WP_ROOT);
}
if (!defined('WPINC')) {
    define('WPINC', 'wp-includes');
}

if (!function_exists('__')) {
    function __($text, $domain = 'default') { return $text; }
}
if (!function_exists('_x')) {
    function _x($text, $context, $domain = 'default') { return $text; }
}
if (!function_exists('_doing_it_wrong')) {
    function _doing_it_wrong($function_name, $message, $version) {
        $GLOBALS['speedyspec_notices'][] = "doing_it_wrong:$function_name|$message|$version";
    }
}
if (!function_exists('_deprecated_function')) {
    function _deprecated_function($function_name, $version, $replacement = '') {
        $GLOBALS['speedyspec_notices'][] = "deprecated_function:$function_name|$version|$replacement";
    }
}
if (!function_exists('_deprecated_argument')) {
    function _deprecated_argument($function_name, $version, $message = '') {
        $GLOBALS['speedyspec_notices'][] = "deprecated_argument:$function_name|$version|$message";
    }
}
if (!function_exists('wp_trigger_error')) {
    function wp_trigger_error($function_name, $message, $error_level = E_USER_NOTICE) {
        $GLOBALS['speedyspec_notices'][] = "trigger_error:$function_name|$message|$error_level";
    }
}
if (!function_exists('get_option')) {
    function get_option($option, $default_value = false) { return 'blog_charset' === $option ? 'UTF-8' : $default_value; }
}
if (!function_exists('is_utf8_charset')) {
    function is_utf8_charset($blog_charset = null) { return true; }
}
if (!function_exists('wp_allowed_protocols')) {
    function wp_allowed_protocols() {
        return ['http', 'https', 'ftp', 'ftps', 'mailto', 'news', 'irc', 'irc6', 'ircs', 'gopher', 'nntp', 'feed',
            'telnet', 'mms', 'rtsp', 'sms', 'svn', 'tel', 'fax', 'xmpp', 'webcal', 'urn'];
    }
}

if (!SPEEDYSPEC_COMPAT_PORT) {
    require SPEEDYSPEC_COMPAT_WP_ROOT . 'wp-includes/plugin.php';
}
foreach (['utf8.php', 'compat-utf8.php', 'class-wp-token-map.php', 'html-api/html5-named-character-references.php',
    'html-api/class-wp-html-decoder.php', 'formatting.php', 'kses.php', 'class-wp-block-type.php',
    'class-wp-block-type-registry.php', 'class-wp-block-styles-registry.php'] as $speedyspec_file) {
    if (is_file(SPEEDYSPEC_COMPAT_WP_ROOT . 'wp-includes/' . $speedyspec_file)) {
        require_once SPEEDYSPEC_COMPAT_WP_ROOT . 'wp-includes/' . $speedyspec_file;
    }
}
if (!function_exists('wp_is_valid_utf8')) {
    function wp_is_valid_utf8($bytes) { return mb_check_encoding($bytes, 'UTF-8'); }
}

/**
 * Defines only the named functions from a release file, so a file that also defines functions this area stubs or
 *  ports can still lend the rest.
 */
function compat_load_functions(string $file, array $names): void
{
    $tokens = token_get_all(file_get_contents(SPEEDYSPEC_COMPAT_WP_ROOT . $file));
    $count = count($tokens);
    for ($i = 0; $i < $count; $i++) {
        if (!is_array($tokens[$i]) || T_FUNCTION !== $tokens[$i][0]) {
            continue;
        }
        $j = $i + 1;
        while ($j < $count && is_array($tokens[$j]) && T_WHITESPACE === $tokens[$j][0]) {
            $j++;
        }
        if (!is_array($tokens[$j]) || T_STRING !== $tokens[$j][0] || !in_array($tokens[$j][1], $names, true)
            || function_exists($tokens[$j][1])) {
            continue;
        }
        $source = '';
        $depth = 0;
        $opened = false;
        for ($k = $i; $k < $count; $k++) {
            $text = is_array($tokens[$k]) ? $tokens[$k][1] : $tokens[$k];
            $source .= $text;
            $opens = '{' === $text
                || (is_array($tokens[$k]) && in_array($tokens[$k][0], [T_CURLY_OPEN, T_DOLLAR_OPEN_CURLY_BRACES], true));
            if ($opens) {
                $depth++;
                $opened = true;
            } elseif ('}' === $text) {
                $depth--;
                if ($opened && 0 === $depth) {
                    break;
                }
            }
        }
        eval($source);
        $i = $k;
    }
}

// WordPress's default filter (default-filters.php), so titles are sanitized as on a real site.
add_filter('sanitize_title', 'sanitize_title_with_dashes', 10, 3);

compat_load_functions('wp-includes/functions.php', ['_wp_array_get', '_wp_array_set', 'wp_is_numeric_array',
    '_wp_to_kebab_case', 'wp_recursive_ksort', 'wp_parse_args', 'wp_json_file_decode', 'mbstring_binary_safe_encoding',
    'reset_mbstring_encoding']);

if (!SPEEDYSPEC_COMPAT_PORT) {
    foreach (['class-wp-theme-json-schema.php', 'class-wp-theme-json.php', 'class-wp-theme-json-data.php'] as $speedyspec_file) {
        require_once SPEEDYSPEC_COMPAT_WP_ROOT . 'wp-includes/' . $speedyspec_file;
    }
    compat_load_functions('wp-includes/global-styles-and-settings.php', ['wp_get_block_css_selector']);
}

/**
 * Called by the driver once it is loaded: reports the notices after each step.
 */
function speedyspec_compat_area_setup()
{
    SpeedySpecCompat::$report_globals = false;
    SpeedySpecCompat::$state = fn() => ['notices' => $GLOBALS['speedyspec_notices']];
}

/**
 * Sorts every array that is not a list by key, recursively, so the fixture compares as PHP's `==` does.
 */
function compat_canon(mixed $value): mixed
{
    if (!is_array($value)) {
        return $value;
    }
    foreach ($value as $k => $v) {
        $value[$k] = compat_canon($v);
    }
    if (!array_is_list($value)) {
        ksort($value, SORT_STRING);
    }
    return $value;
}

/** A theme.json document, built as WordPress builds one. */
function compat_theme_json(array $data = ['version' => 3], string $origin = 'theme'): WP_Theme_JSON
{
    return new WP_Theme_JSON($data, $origin);
}

/** Registers a block type, unregistering any earlier one of the same name. */
function compat_register_block(string $name, array $args = []): void
{
    $registry = WP_Block_Type_Registry::get_instance();
    if ($registry->is_registered($name)) {
        $registry->unregister($name);
    }
    $registry->register($name, $args);
}

/** Registers a block style for a block. */
function compat_register_block_style(string $block, array $style): void
{
    WP_Block_Styles_Registry::get_instance()->register($block, $style);
}

/** A `WP_Theme_JSON` class constant, canonicalised. */
function compat_constant(string $name): mixed
{
    return compat_canon(constant("WP_Theme_JSON::$name"));
}

/** `WP_Theme_JSON_Schema::migrate()`, canonicalised. */
function compat_migrate(mixed $data, string $origin = 'theme'): mixed
{
    return compat_canon(WP_Theme_JSON_Schema::migrate($data, $origin));
}
