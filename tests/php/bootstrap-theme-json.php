<?php

declare(strict_types=1);

/**
 * Loads what WordPress's theme.json tests need around the native theme.json area: the stub test case, the theme.json
 * compat environment on the port's side (the block registries, formatting and kses from the pinned release, the
 * theme, posts and object cache stand-ins), WordPress's notice functions firing the actions the test case listens
 * to, and stand-ins for the theme functions the tests call (`switch_theme()`, theme supports, block registration).
 *
 *  Tests marked `@group needs-wordpress-core` need a booted WordPress (users, the database, the style queue,
 *  translation files) and tests marked `@group documented-break` reach protected members through Reflection (the
 *  design's deliberate difference 2); both are excluded.
 */

require __DIR__ . '/WP_UnitTestCase.php';

if (!function_exists('add_filter') || !class_exists('WP_Theme_JSON', false)) {
    throw new RuntimeException('The native Hook API and theme.json area are not loaded: run these tests in the embedded PHP.');
}

$speedyspec_root = getenv('SPEEDYSPEC_WP_ROOT');
if (!$speedyspec_root) {
    throw new RuntimeException('Set SPEEDYSPEC_WP_ROOT to the prepared WordPress release.');
}
define('SPEEDYSPEC_COMPAT_WP_ROOT', rtrim($speedyspec_root, '/') . '/');
define('SPEEDYSPEC_COMPAT_PORT', true);
define('SPEEDYSPEC_COMPAT_MULTISITE', false);
define('DIR_TESTDATA', __DIR__ . '/wordpress/theme-json/data');
define('WP_CONTENT_DIR', SPEEDYSPEC_COMPAT_WP_ROOT . 'wp-content');

/** WordPress's notice functions fire the actions WP_UnitTestCase listens to (`@expectedDeprecated`, …). */
function _doing_it_wrong($function_name, $message, $version) {
    do_action('doing_it_wrong_run', $function_name, $message, $version);
}
function _deprecated_function($function_name, $version, $replacement = '') {
    do_action('deprecated_function_run', $function_name, $replacement, $version);
}
function _deprecated_argument($function_name, $version, $message = '') {
    do_action('deprecated_argument_run', $function_name, $message, $version);
}
function wp_trigger_error($function_name, $message, $error_level = E_USER_NOTICE) {
    do_action('wp_trigger_error_run', $function_name, $message, $error_level);
}

/** WordPress's test suite sets the development mode through this global (its bootstrap filters it the same way). */
function wp_is_development_mode($mode) {
    $current = $GLOBALS['_wp_tests_development_mode'] ?? '';
    return 'all' === $current || $mode === $current;
}
function is_multisite() { return false; }
define('WP_DEFAULT_THEME', 'default');

require dirname(__DIR__, 2) . '/compat/theme-json/setup.php';

// The style queue, for wp_add_global_styles_for_blocks(): WordPress's own classes and functions.
foreach (['class-wp-dependency.php', 'class-wp-dependencies.php', 'class-wp-styles.php', 'functions.wp-styles.php'] as $speedyspec_file) {
    require_once SPEEDYSPEC_COMPAT_WP_ROOT . 'wp-includes/' . $speedyspec_file;
}
compat_load_functions('wp-includes/script-loader.php', ['wp_should_load_separate_core_block_assets', 'wp_should_load_block_assets_on_demand']);

compat_load_functions('wp-includes/blocks.php', ['register_block_type', 'unregister_block_type', 'unregister_block_style']);

$GLOBALS['wp_theme_directories'] = array(WP_CONTENT_DIR . '/themes');

function get_theme_root($stylesheet_or_template = '') {
    return apply_filters('theme_root', WP_CONTENT_DIR . '/themes', $stylesheet_or_template, '');
}
function wp_clean_themes_cache($clear_update_cache = true) {}

/** Activates a theme from the theme root (or the other theme directories), with its parent from `Template:`. */
function switch_theme($stylesheet) {
    $root = get_theme_root($stylesheet);
    $dir  = $root . '/' . $stylesheet;
    if (!is_dir($dir)) {
        foreach ($GLOBALS['wp_theme_directories'] as $candidate) {
            if (is_dir($candidate . '/' . $stylesheet)) {
                $root = $candidate;
                $dir  = $candidate . '/' . $stylesheet;
                break;
            }
        }
    }
    $template = (new Compat_Theme($dir))->get('Template');
    compat_switch_theme($dir, $template ? $root . '/' . $template : null);
    wp_clean_theme_json_cache();
}

function add_theme_support($feature, ...$args) {
    $GLOBALS['compat_theme_supports'][] = $feature;
    $GLOBALS['compat_theme_support_args'][$feature] = $args ? $args : true;
}
function remove_theme_support($feature) {
    $GLOBALS['compat_theme_supports'] = array_values(array_diff($GLOBALS['compat_theme_supports'] ?? array(), array($feature)));
    unset($GLOBALS['compat_theme_support_args'][$feature]);
    return true;
}

// A booted WordPress registers every core block.
compat_register_core_blocks();
$GLOBALS['wp_theme_directories'][] = DIR_TESTDATA . '/themedir1';
switch_theme('default');

/*
 * Users, as far as these tests need them: an administrator can edit CSS (`edit_css`, single site), others cannot.
 */
function wp_insert_user($userdata) {
    $GLOBALS['compat_users'][] = (array) $userdata;
    return count($GLOBALS['compat_users']);
}
function wp_set_current_user($id, $name = '') {
    $role = $GLOBALS['compat_users'][$id - 1]['role'] ?? '';
    $GLOBALS['compat_current_user'] = (int) $id;
    $GLOBALS['compat_caps'] = 'administrator' === $role ? array('edit_css', 'unfiltered_html', 'manage_options', 'edit_theme_options') : array();
}
function get_current_user_id() {
    return $GLOBALS['compat_current_user'] ?? 0;
}
function _wp_scripts_maybe_doing_it_wrong($function_name, $handle = '') {}

/*
 * What WordPress's database rollback and test case reset between tests: the active theme (its `stylesheet` option),
 * theme supports, posts, the object cache and transients, and the theme.json caches built from them.
 */
function speedyspec_doubles_snapshot() {
    $GLOBALS['speedyspec_snapshot'] = array();
    foreach (array('compat_theme', 'compat_theme_supports', 'compat_theme_support_args', 'compat_posts', 'compat_cache', 'compat_transients') as $speedyspec_global) {
        $GLOBALS['speedyspec_snapshot'][$speedyspec_global] = $GLOBALS[$speedyspec_global] ?? null;
    }
}
function speedyspec_doubles_restore() {
    foreach ($GLOBALS['speedyspec_snapshot'] ?? array() as $speedyspec_global => $speedyspec_value) {
        $GLOBALS[$speedyspec_global] = $speedyspec_value;
    }
    wp_clean_theme_json_cache();
}
function is_admin() { return false; }
