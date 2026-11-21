<?php
/**
 * Loads the WordPress files the i18n corpus needs from SPEEDYSPEC_I18N_ROOT (a stock or prepared release), with
 * stubs for what they reach outside them. record.php and the port's test both use it, so both sides run the same
 * WordPress code around the translation functions.
 */

$root = rtrim(SPEEDYSPEC_I18N_ROOT, '/') . '/';
if (!defined('ABSPATH')) {
    define('ABSPATH', $root);
}
if (!defined('WPINC')) {
    define('WPINC', 'wp-includes');
}
foreach (array('WP_LANG_DIR' => '/languages', 'WP_PLUGIN_DIR' => '/plugins', 'WPMU_PLUGIN_DIR' => '/mu-plugins') as $speedyspec_const => $speedyspec_dir) {
    if (!defined($speedyspec_const)) {
        define($speedyspec_const, SPEEDYSPEC_I18N_DATA . $speedyspec_dir);
    }
}
if (!defined('WP_CONTENT_DIR')) {
    define('WP_CONTENT_DIR', SPEEDYSPEC_I18N_DATA);
}
if (!defined('WP_CONTENT_URL')) {
    define('WP_CONTENT_URL', 'http://example.org/wp-content');
}
if (!defined('WP_PLUGIN_URL')) {
    define('WP_PLUGIN_URL', WP_CONTENT_URL . '/plugins');
}
if (!defined('HOUR_IN_SECONDS')) {
    define('HOUR_IN_SECONDS', 3600);
}
error_reporting(E_ALL & ~E_DEPRECATED & ~E_WARNING & ~E_NOTICE);

if (!function_exists('get_option')) {
    function get_option($option, $default_value = false) {
        $options = array('blog_charset' => 'UTF-8', 'siteurl' => 'http://example.org', 'home' => 'http://example.org');
        return $options[$option] ?? $default_value;
    }
}
if (!function_exists('get_site_option')) {
    function get_site_option($option, $default_value = false) { return $default_value; }
}
if (!function_exists('_doing_it_wrong')) {
    function _doing_it_wrong($function_name, $message, $version) { $GLOBALS['speedyspec_wrong'][] = $function_name; }
}
if (!function_exists('get_theme_root')) {
    // WordPress's answer with no extra theme directories registered.
    function get_theme_root($stylesheet_or_template = '') { return apply_filters('theme_root', WP_CONTENT_DIR . '/themes'); }
}
if (!function_exists('wp_normalize_path')) {
    // WordPress's wp_normalize_path() for paths without a stream wrapper (functions.php is not loaded).
    function wp_normalize_path($path) {
        $path = (string) preg_replace('|(?<=.)/+|', '/', str_replace('\\', '/', (string) $path));
        return ':' === substr($path, 1, 1) ? ucfirst($path) : $path;
    }
}
if (!function_exists('_deprecated_function')) {
    function _deprecated_function($function_name, $version, $replacement = '') {}
}
if (!function_exists('_deprecated_argument')) {
    function _deprecated_argument($function_name, $version, $message = '') {}
}

$files = array('load.php', 'compat.php', 'compat-utf8.php', 'utf8.php', 'cache.php', 'pomo/translations.php', 'pomo/mo.php',
    'l10n/class-wp-translation-controller.php', 'l10n/class-wp-translations.php', 'l10n/class-wp-translation-file.php',
    'l10n/class-wp-translation-file-mo.php', 'l10n/class-wp-translation-file-php.php', 'l10n.php',
    'class-wp-textdomain-registry.php', 'formatting.php', 'http.php', 'link-template.php', 'class-wp-dependency.php',
    'class-wp-dependencies.php', 'class-wp-scripts.php', 'functions.wp-scripts.php', 'class-wp-script-modules.php',
    'script-modules.php');
if (!function_exists('add_filter')) {
    array_unshift($files, 'plugin.php');
}
foreach ($files as $f) {
    require_once $root . WPINC . '/' . $f;
}
wp_cache_init();
$GLOBALS['wp_textdomain_registry'] = new WP_Textdomain_Registry();
