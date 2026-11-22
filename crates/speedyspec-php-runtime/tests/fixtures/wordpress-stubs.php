<?php
// The few WordPress functions the native option functions call (functions.php is not loaded), as in
// speedyspec-wp-options-php/tests/php.rs.
function wp_installing($is_installing = null) {
    static $installing = false;
    if (null !== $is_installing) { $old = $installing; $installing = (bool) $is_installing; return $old; }
    return $installing;
}
function is_multisite() { return false; }
function get_current_blog_id() { return 1; }
function get_current_network_id() { return 1; }
function __($text, $domain = 'default') { return $text; }
function esc_html($text) { return htmlspecialchars((string) $text, ENT_QUOTES); }
function esc_attr($text) { return htmlspecialchars((string) $text, ENT_QUOTES); }
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
