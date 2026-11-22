<?php

declare(strict_types=1);

/**
 * Sets up the shortcode scenarios the same way on both sides: WordPress's formatting and kses files, the stubs of
 *  the few WordPress functions they reach, WordPress's test handlers, and the state each step reports.
 *
 *  `SPEEDYSPEC_COMPAT_WP_ROOT` is the release to load from and `SPEEDYSPEC_COMPAT_PORT` whether the native port is
 *  loaded. The recorder loads WordPress's `plugin.php` and `shortcodes.php`; the port's side loads neither (they are
 *  native). Both load `formatting.php` and `kses.php`, which `do_shortcodes_in_html_tags()` calls on both sides; the
 *  port's release is prepared by speedyspec-wp-kses-php, so its `kses.php` runs only for the globals and the kses
 *  functions are native.
 */

$GLOBALS['speedyspec_notices'] = [];

if (!function_exists('__')) {
    function __($text, $domain = 'default') { return $text; }
}
if (!function_exists('_doing_it_wrong')) {
    function _doing_it_wrong($function_name, $message, $version) {
        $GLOBALS['speedyspec_notices'][] = "doing_it_wrong:$function_name|$message|$version";
    }
}
if (!function_exists('__return_false')) {
    function __return_false() { return false; }
}
if (!function_exists('wp_allowed_protocols')) {
    function wp_allowed_protocols() {
        return ['http', 'https', 'ftp', 'ftps', 'mailto', 'news', 'irc', 'irc6', 'ircs', 'gopher', 'nntp', 'feed',
            'telnet', 'mms', 'rtsp', 'sms', 'svn', 'tel', 'fax', 'xmpp', 'webcal', 'urn'];
    }
}
if (!function_exists('get_option')) {
    function get_option($option, $default_value = false) { return 'blog_charset' === $option ? 'UTF-8' : $default_value; }
}
if (!function_exists('is_utf8_charset')) {
    function is_utf8_charset($blog_charset = null) { return true; }
}

if (!SPEEDYSPEC_COMPAT_PORT) {
    require SPEEDYSPEC_COMPAT_WP_ROOT . 'wp-includes/plugin.php';
    require SPEEDYSPEC_COMPAT_WP_ROOT . 'wp-includes/shortcodes.php';
}
foreach (['utf8.php', 'compat-utf8.php', 'class-wp-token-map.php', 'html-api/html5-named-character-references.php',
    'html-api/class-wp-html-decoder.php', 'formatting.php', 'kses.php'] as $speedyspec_file) {
    if (is_file(SPEEDYSPEC_COMPAT_WP_ROOT . 'wp-includes/' . $speedyspec_file)) {
        require_once SPEEDYSPEC_COMPAT_WP_ROOT . 'wp-includes/' . $speedyspec_file;
    }
}
if (!function_exists('wp_is_valid_utf8')) {
    function wp_is_valid_utf8($bytes) { return mb_check_encoding($bytes, 'UTF-8'); }
}

set_error_handler(function ($errno, $message) {
    if (E_DEPRECATED === $errno || E_WARNING === $errno) {
        $GLOBALS['speedyspec_notices'][] = (E_DEPRECATED === $errno ? 'deprecated:' : 'warning:') . $message;
        return true;
    }
    return false;
});

/**
 * Called by the driver once it is loaded: reports the registry's names and the notices after each step.
 */
function speedyspec_compat_area_setup()
{
    SpeedySpecCompat::$report_globals = false;
    SpeedySpecCompat::$state = fn() => [
        'tags' => array_keys($GLOBALS['shortcode_tags'] ?? []),
        'notices' => $GLOBALS['speedyspec_notices'],
    ];
}

function compat_ser_atts($atts) { $o = ''; foreach ((array) $atts as $k => $v) { $o .= (is_int($k) ? "i$k" : "s$k") . '=' . $v . ';'; } return $o; }
function compat_h_tst($atts, $content = null, $tag = null) { return 'T{' . compat_ser_atts($atts) . '|' . (null === $content ? 'NULL' : 'C:' . $content) . '|' . $tag . '}'; }
function compat_h_core($atts, $content = null, $tag = null) { return 'X{' . $tag . '|' . compat_ser_atts($atts) . '|' . (null === $content ? 'NULL' : 'C:' . $content) . '}'; }
function compat_h_foo($atts) { $foo = $atts['foo'] ?? ''; return "foo = $foo"; }
function compat_h_bar($atts) { $p = shortcode_atts(['foo' => 'no foo', 'baz' => 'default baz'], $atts, 'bartag'); return "foo = {$p['foo']}"; }
function compat_h_baz($atts, $content = '') { return 'content = ' . do_shortcode($content); }
function compat_h_dump($atts) { $o = ''; foreach ($atts as $k => $v) { $o .= "$k = $v\n"; } return $o; }
function compat_h_hyphen() { return 'shortcode_hyphen'; }
function compat_h_hyphen_foo() { return 'shortcode_hyphen_foo'; }
function compat_h_hyphen_foo_bar() { return 'shortcode_hyphen_foo_bar'; }
function compat_h_url() { return 'http://www.wordpress.org/'; }
function compat_h_img($atts) { $o = '<img'; foreach ($atts as $k => $v) { $o .= " $k=\"$v\""; } return $o . ' />'; }
function compat_h_array() { return [1]; }
function compat_h_throw() { throw new RuntimeException('handler threw'); }
function compat_h_reenter($atts, $content = '') { remove_shortcode('reenter'); return do_shortcode((string) $content); }

/**
 * Registers WordPress's core shortcodes (as stubs) and its tests' handlers, in WordPress's order.
 */
function compat_register()
{
    remove_all_shortcodes();
    foreach (['wp_caption', 'caption', 'gallery', 'playlist', 'audio', 'video', 'embed'] as $tag) {
        add_shortcode($tag, 'compat_h_core');
    }
    $handlers = ['test-shortcode-tag' => 'compat_h_tst', 'footag' => 'compat_h_foo', 'bartag' => 'compat_h_bar',
        'baztag' => 'compat_h_baz', 'dumptag' => 'compat_h_dump', 'hyphen' => 'compat_h_hyphen',
        'hyphen-foo' => 'compat_h_hyphen_foo', 'hyphen-foo-bar' => 'compat_h_hyphen_foo_bar', 'url' => 'compat_h_url',
        'img' => 'compat_h_img'];
    foreach ($handlers as $tag => $handler) {
        add_shortcode($tag, $handler);
    }
}

/** `do_shortcode()` on hex-encoded content, so invalid UTF-8 survives the JSON on both sides. */
function compat_do_hex($hex, $ignore_html = false) { return bin2hex(do_shortcode(hex2bin($hex), $ignore_html)); }

/** `shortcode_parse_atts()` on hex-encoded text, with each value hex-encoded. */
function compat_parse_hex($hex) { return array_map('bin2hex', shortcode_parse_atts(hex2bin($hex))); }

function compat_filter_bar() { return 'bar'; }
function compat_filter_p11() { return 'p11'; }
function compat_filter_attr($output, $key, $atts, $m) { return json_encode(['output' => $output, 'key' => $key, 'atts' => $atts, 'm' => $m]); }
function compat_filter_replace($output) { return str_replace('oo', 'ee', $output); }
function compat_filter_generate() { return 'foobar'; }
function compat_filter_atts2($out, $pairs, $atts) {
    if (isset($out['foo']) && 'foo1' === $out['foo']) { $out['foo'] = $pairs['foo']; }
    if (isset($out['baz'])) { unset($out['baz']); }
    return $out;
}
function compat_filter_tagnames() { return ['gallery']; }
function compat_filter_array() { return [1]; }
function compat_context() { return apply_filters('wp_get_attachment_image_context', 'x'); }
function compat_h_context() { return compat_context() . '|' . (has_filter('wp_get_attachment_image_context', '_filter_do_shortcode_context') ? 'attached' : 'not'); }
