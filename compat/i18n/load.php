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
define('WP_LANG_DIR', SPEEDYSPEC_I18N_DATA . '/languages');
define('WP_PLUGIN_DIR', SPEEDYSPEC_I18N_DATA . '/plugins');
define('WPMU_PLUGIN_DIR', SPEEDYSPEC_I18N_DATA . '/mu-plugins');
define('HOUR_IN_SECONDS', 3600);
error_reporting(E_ALL & ~E_DEPRECATED & ~E_WARNING & ~E_NOTICE);

function get_option($option, $default_value = false) { return 'blog_charset' === $option ? 'UTF-8' : $default_value; }
function get_site_option($option, $default_value = false) { return $default_value; }
function _doing_it_wrong($function_name, $message, $version) { $GLOBALS['speedyspec_wrong'][] = $function_name; }
function _deprecated_function($function_name, $version, $replacement = '') {}
function _deprecated_argument($function_name, $version, $message = '') {}

$files = array('load.php', 'compat.php', 'compat-utf8.php', 'utf8.php', 'cache.php', 'pomo/translations.php', 'pomo/mo.php',
    'l10n/class-wp-translation-controller.php', 'l10n/class-wp-translations.php', 'l10n/class-wp-translation-file.php',
    'l10n/class-wp-translation-file-mo.php', 'l10n/class-wp-translation-file-php.php', 'l10n.php',
    'class-wp-textdomain-registry.php');
if (!function_exists('add_filter')) {
    array_unshift($files, 'plugin.php');
}
foreach ($files as $f) {
    require_once $root . WPINC . '/' . $f;
}
wp_cache_init();
$GLOBALS['wp_textdomain_registry'] = new WP_Textdomain_Registry();
