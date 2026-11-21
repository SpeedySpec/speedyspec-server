<?php
/**
 * Loads WordPress's token map from SPEEDYSPEC_TOKEN_MAP_ROOT (a stock or prepared release) with stubs for what it
 * reaches outside itself. record.php and the port's test both use it, so both sides run the same code around it.
 */

$speedyspec_root = rtrim(SPEEDYSPEC_TOKEN_MAP_ROOT, '/') . '/';
if (!defined('ABSPATH')) {
    define('ABSPATH', $speedyspec_root);
}
if (!defined('WPINC')) {
    define('WPINC', 'wp-includes');
}
if (!function_exists('__')) {
    function __($text, $domain = 'default') { return $text; }
}
if (!function_exists('_doing_it_wrong')) {
    function _doing_it_wrong($function_name, $message, $version) { $GLOBALS['speedyspec_wrong'][] = array($function_name, $message); }
}
require_once $speedyspec_root . WPINC . '/class-wp-token-map.php';
require_once $speedyspec_root . WPINC . '/html-api/html5-named-character-references.php';
