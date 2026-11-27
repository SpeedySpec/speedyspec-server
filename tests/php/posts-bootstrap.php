<?php

declare(strict_types=1);

/**
 * Loads what WordPress's post-reading tests need around the native post functions: the stub test case and mocks,
 *  then the post-reading compatibility setup with the port loaded (WordPress's formatting, option, meta, post,
 *  taxonomy, user and capability files over MySQL, with the core post types and the category and tag taxonomies).
 *
 *  Beside it: WordPress's rewrite API and `WP` (for the query vars and rewrite rules `register_post_type()` adds),
 *  with a permalink structure; and the REST controller classes `WP_Post_Type` hands out.
 *
 *  The factory stores the posts the tests make as rows (`SPEEDYSPEC_FACTORY_ROWS`, see includes/factory.php).
 *  `WP_TESTS_MULTISITE=1` makes the install a multisite one through the setup's multisite branch.
 *
 *  As WordPress's own bootstrap and test case do: the seeded posts are deleted before the run (`_delete_all_data()`),
 *  each test starts with the core post types, no custom post statuses and the bootstrap's taxonomies, the post loop
 *  globals are emptied after each test, and every test class's data is deleted after it.
 */

require __DIR__ . '/WP_UnitTestCase.php';
require __DIR__ . '/includes/mock-action.php';
require __DIR__ . '/includes/mock-invokable.php';

if (!function_exists('get_post') || !class_exists('wpdb'))
{
    throw new RuntimeException('The native post functions are not loaded: run these tests in the embedded PHP.');
}

$speedyspec_root = getenv('SPEEDYSPEC_POSTS_WP_ROOT');
if (!$speedyspec_root)
{
    throw new RuntimeException('Set SPEEDYSPEC_POSTS_WP_ROOT to the prepared WordPress release.');
}
define('SPEEDYSPEC_COMPAT_WP_ROOT', rtrim($speedyspec_root, '/') . '/');
define('SPEEDYSPEC_COMPAT_PORT', true);
define('SPEEDYSPEC_COMPAT_MULTISITE', '1' === getenv('WP_TESTS_MULTISITE'));
define('SPEEDYSPEC_FACTORY_ROWS', true);
define('DIR_TESTDATA', __DIR__ . '/data');
require dirname(__DIR__, 2) . '/compat/posts-read/setup.php';

// The rewrite API and the query vars, as wp-settings.php loads and sets them up, with WordPress's tests' default
// permalink structure (WP_TESTS_DEFAULT_PERMALINK_STRUCTURE).
require_once ABSPATH . WPINC . '/class-wp-rewrite.php';
require_once ABSPATH . WPINC . '/rewrite.php';
require_once ABSPATH . WPINC . '/class-wp.php';
update_option('permalink_structure', '/%year%/%monthnum%/%day%/%postname%/');
$GLOBALS['wp_rewrite'] = new WP_Rewrite();
$GLOBALS['wp'] = new WP();

// The REST controllers WP_Post_Type::get_rest_controller() and its revisions and autosave readers construct.
foreach ([
    'endpoints/class-wp-rest-controller.php',
    'endpoints/class-wp-rest-posts-controller.php',
    'endpoints/class-wp-rest-revisions-controller.php',
    'endpoints/class-wp-rest-autosaves-controller.php',
    'endpoints/class-wp-rest-template-revisions-controller.php',
    'endpoints/class-wp-rest-template-autosaves-controller.php',
    'fields/class-wp-rest-meta-fields.php',
    'fields/class-wp-rest-post-meta-fields.php',
    'fields/class-wp-rest-revision-meta-fields.php',
] as $speedyspec_file)
{
    require_once ABSPATH . WPINC . '/rest-api/' . $speedyspec_file;
}

/**
 * Unregisters a post type, as WordPress's tests/phpunit/includes/utils.php does (the built-in types refuse).
 */
function _unregister_post_type($cpt_name)
{
    unregister_post_type($cpt_name);
}

/**
 * Removes a post status from the registry, as WordPress's tests/phpunit/includes/utils.php does.
 */
function _unregister_post_status($status)
{
    unset($GLOBALS['wp_post_statuses'][$status]);
}

/**
 * Deletes every post, meta row, term relationship and user but the first, as WordPress's
 *  tests/phpunit/includes/utils.php does (over the tables compat/posts-read/setup.php creates).
 */
function _delete_all_data()
{
    global $wpdb;
    foreach ([$wpdb->posts, $wpdb->postmeta, $wpdb->term_relationships, $wpdb->termmeta] as $table)
    {
        $wpdb->query("DELETE FROM {$table}");
    }
    foreach ([$wpdb->terms, $wpdb->term_taxonomy] as $table)
    {
        $wpdb->query("DELETE FROM {$table} WHERE term_id != 1");
    }
    $wpdb->query("UPDATE {$wpdb->term_taxonomy} SET count = 0");
    $wpdb->query("DELETE FROM {$wpdb->users} WHERE ID != 1");
    $wpdb->query("DELETE FROM {$wpdb->usermeta} WHERE user_id != 1");
}

/** @var array<string, WP_Taxonomy> The taxonomies as the bootstrap registered them, restored before each test. */
$GLOBALS['speedyspec_taxonomies'] = array_map(static fn ($taxonomy) => clone $taxonomy, $GLOBALS['wp_taxonomies']);

/**
 * WordPress's test case `set_up()` resets: the core post types (`reset_post_types()`), no custom post statuses
 *  (`reset_post_statuses()`), the taxonomies (`reset_taxonomies()`, here the bootstrap's own), and the rewrite
 *  rules and endpoints earlier tests added (`$wp_rewrite->init()`).
 */
function speedyspec_doubles_snapshot(): void
{
    foreach (get_post_types([], 'objects') as $post_type)
    {
        if (empty($post_type->tests_no_auto_unregister))
        {
            _unregister_post_type($post_type->name);
        }
    }
    create_initial_post_types();
    foreach (get_post_stati(['_builtin' => false]) as $post_status)
    {
        _unregister_post_status($post_status);
    }
    $GLOBALS['wp_taxonomies'] = array_map(static fn ($taxonomy) => clone $taxonomy, $GLOBALS['speedyspec_taxonomies']);
    $GLOBALS['wp_rewrite']->init();
}

/**
 * WordPress's test case `tear_down()` makes a new `WP` and empties the post loop globals `setup_postdata()` writes.
 */
function speedyspec_doubles_restore(): void
{
    $GLOBALS['wp'] = new WP();
    foreach (['post', 'id', 'authordata', 'currentday', 'currentmonth', 'page', 'pages', 'multipage', 'more', 'numpages'] as $global)
    {
        $GLOBALS[$global] = null;
    }
}

// WordPress's own bootstrap deletes the posts the install made; the run starts with none.
_delete_all_data();
wp_cache_flush();
