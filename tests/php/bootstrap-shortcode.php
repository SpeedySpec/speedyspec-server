<?php

declare(strict_types=1);

/**
 * Loads what WordPress's shortcode tests need around the native Shortcode, kses and Hook APIs: the stub test case,
 * WordPress's formatting and kses files from the pinned release (`SPEEDYSPEC_WP_ROOT`, prepared by
 * speedyspec-wp-kses-php, so kses.php runs only for its globals), and the few other WordPress functions the tests
 * call.
 *
 *  Tests marked `@group needs-wordpress-core` need a booted WordPress (the `the_content` filter chain) and are
 *  excluded here.
 */

require __DIR__ . '/WP_UnitTestCase.php';

if (!function_exists('do_shortcode') || !function_exists('wp_kses_one_attr') || !function_exists('add_filter')) {
    throw new RuntimeException('The native Shortcode, kses and Hook APIs are not loaded: run these tests in the embedded PHP.');
}

$speedyspec_root = getenv('SPEEDYSPEC_WP_ROOT');
if (!$speedyspec_root) {
    throw new RuntimeException('Set SPEEDYSPEC_WP_ROOT to the WordPress release the tests load formatting.php from.');
}
define('ABSPATH', rtrim($speedyspec_root, '/') . '/');
define('WPINC', 'wp-includes');
define('DIR_TESTDATA', __DIR__ . '/wordpress/shortcode/data');

function __($text, $domain = 'default') { return $text; }
function __return_false() { return false; }
function wp_json_encode($value, $flags = 0, $depth = 512) { return json_encode($value, $flags, $depth); }
function get_option($option, $default_value = false) { return 'blog_charset' === $option ? 'UTF-8' : $default_value; }
function is_utf8_charset($blog_charset = null) { return true; }

/**
 * Fires `doing_it_wrong_run`, as WordPress's `_doing_it_wrong()` does, so `@expectedIncorrectUsage` works.
 */
function _doing_it_wrong($function_name, $message, $version) {
    do_action('doing_it_wrong_run', $function_name, $message, $version);
}

foreach (['utf8.php', 'compat-utf8.php', 'class-wp-token-map.php', 'html-api/html5-named-character-references.php',
    'html-api/class-wp-html-decoder.php', 'formatting.php', 'kses.php'] as $speedyspec_file) {
    if (is_file(ABSPATH . WPINC . '/' . $speedyspec_file)) {
        require_once ABSPATH . WPINC . '/' . $speedyspec_file;
    }
}
if (!function_exists('wp_is_valid_utf8')) {
    function wp_is_valid_utf8($bytes) { return mb_check_encoding($bytes, 'UTF-8'); }
}

/**
 * Removes leading and trailing whitespace from each line and blank lines (WordPress's tests/phpunit/includes/utils.php).
 */
function strip_ws($txt) {
    $lines  = explode("\n", $txt);
    $result = array();
    foreach ($lines as $line) {
        if (trim($line)) {
            $result[] = trim($line);
        }
    }
    return trim(implode("\n", $result));
}

/**
 * Finds the backtrack limit a pattern needs on a subject (WordPress's tests/phpunit/includes/utils.php).
 */
function benchmark_pcre_backtracking($pattern, $subject, $strategy) {
    $saved_config = ini_get('pcre.backtrack_limit');
    $limit = 1000000;
    for ($i = 4; $i <= $limit; $i *= 2) {
        ini_set('pcre.backtrack_limit', (string) $i);
        switch ($strategy) {
            case 'split':
                preg_split($pattern, $subject);
                break;
            case 'match':
                preg_match($pattern, $subject);
                break;
            case 'match_all':
                $matches = array();
                preg_match_all($pattern, $subject, $matches);
                break;
        }
        ini_set('pcre.backtrack_limit', $saved_config);
        switch (preg_last_error()) {
            case PREG_NO_ERROR:
                return $i;
            case PREG_BACKTRACK_LIMIT_ERROR:
                break;
            case PREG_RECURSION_LIMIT_ERROR:
                trigger_error('PCRE recursion limit encountered before backtrack limit.');
                return;
            case PREG_BAD_UTF8_ERROR:
                trigger_error('UTF-8 error during PCRE benchmark.');
                return;
            case PREG_INTERNAL_ERROR:
                trigger_error('Internal error during PCRE benchmark.');
                return;
            default:
                trigger_error('Unexpected error during PCRE benchmark.');
                return;
        }
    }
    return $i;
}

/**
 * A random string (WordPress's tests/phpunit/includes/utils.php).
 */
function rand_str($length = 32) {
    return substr(md5(uniqid((string) rand())), 0, $length);
}

function current_theme_supports($feature, ...$args) { return false; }
function wp_unique_prefixed_id($prefix = '') { static $id = 0; return $prefix . (string) ++$id; }

// A booted WordPress registers its core shortcodes; media.php registers its own (the caption output is tested), and
// the rest are stand-ins, since the tests only need them to exist.
require_once ABSPATH . WPINC . '/media.php';
// gallery_shortcode() reads posts and attachments; with none, as here, WordPress's own output is ''.
function speedyspec_empty_gallery() { return ''; }
add_shortcode('gallery', 'speedyspec_empty_gallery');
foreach (['wp_caption', 'caption', 'gallery', 'playlist', 'audio', 'video', 'embed'] as $speedyspec_tag) {
    if (!shortcode_exists($speedyspec_tag)) {
        add_shortcode($speedyspec_tag, '__return_false');
    }
}
