<?php
// The few WordPress functions the native option functions call (functions.php is not loaded), as in
// speedyspec-wp-options-php/tests/php.rs.
function wp_installing($is_installing = null) {
    static $installing = false;
    if (null !== $is_installing) { $old = $installing; $installing = (bool) $is_installing; return $old; }
    return $installing;
}
// A fixture that boots a network declares these first.
if (!function_exists('is_multisite')) { function is_multisite() { return false; } }
if (!function_exists('get_current_blog_id')) { function get_current_blog_id() { return 1; } }
function get_current_network_id() { return 1; }
// __(), esc_html() and esc_attr() are the runtime's own (the i18n and text ports). The i18n port's uncommon paths
// still call WordPress's get_translations_for_domain(), which no port declares yet: a domain with nothing loaded
// answers as WordPress's NOOP_Translations does.
if (!function_exists('get_translations_for_domain')) {
    function get_translations_for_domain($domain) {
        return new class {
            public function translate($singular, $context = null) { return $singular; }
            public function translate_plural($singular, $plural, $count, $context = null) {
                return 1 === (int) $count ? $singular : $plural;
            }
        };
    }
}
function _deprecated_argument($function_name, $version, $message = '') {
    do_action('deprecated_argument_run', $function_name, $message, $version);
}
function wp_die($message = '') {
    $handler = apply_filters('wp_die_handler', '_default_wp_die_handler');
    call_user_func($handler, $message);
}
function _default_wp_die_handler($message) { throw new Error('wp_die: ' . $message); }
function wp_using_ext_object_cache($using = null) {
    global $_wp_using_ext_object_cache;
    $current_using = $_wp_using_ext_object_cache;
    if (null !== $using) { $_wp_using_ext_object_cache = $using; }
    return $current_using;
}
function is_main_site($site_id = null, $network_id = null) { return true; }
function is_main_network($network_id = null) { return true; }
function _doing_it_wrong($function_name, $message, $version) {
    do_action('doing_it_wrong_run', $function_name, $message, $version);
}
function wp_parse_args($args, $defaults = array()) {
    if (is_object($args)) { $parsed_args = get_object_vars($args); }
    elseif (is_array($args)) { $parsed_args =& $args; }
    else { parse_str((string) $args, $parsed_args); }
    if (is_array($defaults) && $defaults) { return array_merge($defaults, $parsed_args); }
    return $parsed_args;
}
