<?php

declare(strict_types=1);

/**
 * Loads what WordPress's post-reading tests need around the native post functions: the stub test case and mocks,
 *  then the post-reading compatibility setup with the port loaded (WordPress's formatting, option, meta, post,
 *  taxonomy, user and capability files over MySQL, with the core post types and the category and tag taxonomies).
 *
 *  Beside it: WordPress's rewrite API and `WP` (for the query vars and rewrite rules `register_post_type()` adds),
 *  with a permalink structure; the REST controller classes `WP_Post_Type` hands out; and what post.php's write path
 *  (`wp_insert_post()`, `wp_trash_post()`, `wp_delete_post()`) calls: WordPress's comment.php, cron.php and
 *  link-template.php, default-constants.php's time constants and `EMPTY_TRASH_DAYS`, and the comment tables. The
 *  write path is WordPress's own and not ported; it makes the fixtures the reading tests read. A save of a post with
 *  a name runs `get_posts()`, so WordPress's own `WP_Query` is loaded too, as the runtime serves it until the query
 *  port, with wp-admin's template.php for `_post_states()`. Also: post-template.php (`get_post_parent()`), and the HTML
 *  API and block parser for the tests' `assertEqualHTML()`.
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
define('SPEEDYSPEC_FACTORY_ROLES', true);
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

// What post.php's write path calls: default comment statuses and comment deletion, scheduled-hook clean-up, and
// permalinks (for an empty guid), with the trash lifetime default-constants.php gives.
require_once ABSPATH . WPINC . '/comment.php';
require_once ABSPATH . WPINC . '/cron.php';
require_once ABSPATH . WPINC . '/link-template.php';
// default-constants.php's wp_initial_constants(): the time units wp_insert_post() compares dates in, and the trash
// lifetime.
foreach ([
    'MINUTE_IN_SECONDS' => 60,
    'HOUR_IN_SECONDS' => 3600,
    'DAY_IN_SECONDS' => 86400,
    'WEEK_IN_SECONDS' => 604800,
    'MONTH_IN_SECONDS' => 2592000,
    'YEAR_IN_SECONDS' => 31536000,
    'EMPTY_TRASH_DAYS' => 30,
] as $speedyspec_name => $speedyspec_value)
{
    if (!defined($speedyspec_name))
    {
        define($speedyspec_name, $speedyspec_value);
    }
}
// get_post_parent() and has_post_parent(), WordPress's own template functions over the native readers.
require_once ABSPATH . WPINC . '/post-template.php';
// The HTML API, for the tests' build_visual_html_tree() (assertEqualHTML()).
require_once ABSPATH . WPINC . '/class-wp-token-map.php';
foreach ([
    'html5-named-character-references', 'class-wp-html-attribute-token', 'class-wp-html-span',
    'class-wp-html-doctype-info', 'class-wp-html-text-replacement', 'class-wp-html-decoder',
    'class-wp-html-tag-processor', 'class-wp-html-unsupported-exception', 'class-wp-html-active-formatting-elements',
    'class-wp-html-open-elements', 'class-wp-html-token', 'class-wp-html-stack-event',
    'class-wp-html-processor-state', 'class-wp-html-processor',
] as $speedyspec_file)
{
    require_once ABSPATH . WPINC . "/html-api/{$speedyspec_file}.php";
}
// The block parser build_visual_html_tree() reads block comments with.
require_once ABSPATH . WPINC . '/class-wp-block-processor.php';
require_once ABSPATH . WPINC . '/class-wp-block-parser.php';
require_once __DIR__ . '/includes/build-visual-html-tree.php';
$GLOBALS['wpdb']->query(
    "CREATE TABLE IF NOT EXISTS {$GLOBALS['wpdb']->comments} (comment_ID bigint(20) unsigned NOT NULL auto_increment, "
    . "comment_post_ID bigint(20) unsigned NOT NULL default '0', comment_author tinytext NOT NULL, "
    . "comment_author_email varchar(100) NOT NULL default '', comment_author_url varchar(200) NOT NULL default '', "
    . "comment_author_IP varchar(100) NOT NULL default '', "
    . "comment_date datetime NOT NULL default '0000-00-00 00:00:00', "
    . "comment_date_gmt datetime NOT NULL default '0000-00-00 00:00:00', comment_content text NOT NULL, "
    . "comment_karma int(11) NOT NULL default '0', comment_approved varchar(20) NOT NULL default '1', "
    . "comment_agent varchar(255) NOT NULL default '', comment_type varchar(20) NOT NULL default 'comment', "
    . "comment_parent bigint(20) unsigned NOT NULL default '0', user_id bigint(20) unsigned NOT NULL default '0', "
    . "PRIMARY KEY (comment_ID), KEY comment_post_ID (comment_post_ID)) "
    . "DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_520_ci"
);
$GLOBALS['wpdb']->query(
    "CREATE TABLE IF NOT EXISTS {$GLOBALS['wpdb']->commentmeta} (meta_id bigint(20) unsigned NOT NULL auto_increment, "
    . "comment_id bigint(20) unsigned NOT NULL default '0', meta_key varchar(255) default NULL, meta_value longtext, "
    . "PRIMARY KEY (meta_id), KEY comment_id (comment_id), KEY meta_key (meta_key(191))) "
    . "DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_520_ci"
);

// WordPress's default roles, as the install makes them (wp-admin's populate_roles()), in place of the seed's few
// capabilities: the write path checks them (a pending post's slug needs `publish_pages`).
require_once ABSPATH . 'wp-admin/includes/schema.php';
delete_option('wp_user_roles');
$GLOBALS['wp_roles'] = null;
populate_roles();
wp_roles()->for_site();
// default-constants.php's content URL, which the attachment URL wp_trash_post() records is built on.
if (!defined('WP_CONTENT_URL'))
{
    define('WP_CONTENT_URL', get_option('siteurl') . '/wp-content');
}

// WordPress's own WP_Query, which the runtime serves until the query port: get_posts(), and the trashed-suffix check
// wp_insert_post() runs for a post with a name. The main query globals as wp-settings.php sets them.
require_once ABSPATH . WPINC . '/class-wp-date-query.php';
require_once ABSPATH . WPINC . '/class-wp-tax-query.php';
require_once ABSPATH . WPINC . '/class-wp-query.php';
require_once ABSPATH . WPINC . '/query.php';
$GLOBALS['wp_the_query'] = new WP_Query();
$GLOBALS['wp_query'] = $GLOBALS['wp_the_query'];
// _post_states() and get_post_states(), wp-admin's.
require_once ABSPATH . WPINC . '/class-wp-walker.php';
require_once ABSPATH . 'wp-admin/includes/template.php';

/**
 * Gives the list separator as l10n.php's does with no locale loaded (compat/posts-read/setup.php stands in for
 *  l10n.php's translation functions, so l10n.php itself is not loaded); _post_states() joins with it.
 */
function wp_get_list_item_separator()
{
    return __(', ');
}

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
 * Deletes every post, comment, meta row, term relationship and user but the first, as WordPress's
 *  tests/phpunit/includes/functions.php does.
 *
 *  Unlike WordPress's, it deletes attachment rows without `wp_delete_attachment()`: the factory uploads no files.
 */
function _delete_all_data()
{
    global $wpdb;
    foreach ([$wpdb->posts, $wpdb->postmeta, $wpdb->comments, $wpdb->commentmeta, $wpdb->term_relationships,
        $wpdb->termmeta] as $table)
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
 * WordPress's test case `tear_down()` makes a new main query and `WP`, and empties the post loop globals
 *  `setup_postdata()` writes.
 */
function speedyspec_doubles_restore(): void
{
    $GLOBALS['wp_the_query'] = new WP_Query();
    $GLOBALS['wp_query'] = $GLOBALS['wp_the_query'];
    $GLOBALS['wp'] = new WP();
    $globals = ['post', 'id', 'authordata', 'currentday', 'currentmonth', 'page', 'pages', 'multipage', 'more',
        'numpages'];
    foreach ($globals as $global)
    {
        $GLOBALS[$global] = null;
    }
}

// WordPress's own bootstrap deletes the posts the install made; the run starts with none.
_delete_all_data();
wp_cache_flush();
