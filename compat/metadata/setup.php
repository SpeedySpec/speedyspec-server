<?php

declare(strict_types=1);

/**
 * Sets up the metadata scenarios the same way on both sides: WordPress's own files, the same tables and seeds, and
 *  the state each step reports.
 *
 *  `SPEEDYSPEC_COMPAT_WP_ROOT` is the release to load from and `SPEEDYSPEC_COMPAT_PORT` whether the native port is
 *  loaded. The recorder loads WordPress's hooks, object cache, `wpdb` and `meta.php`; the port's side loads none of
 *  the first three (they are native) and its release copy's `meta.php` has the ported functions removed (and
 *  `class-wp-metadata-lazyloader.php`, which it requires, replaced). Both need MySQL, through
 *  `SPEEDYSPEC_TEST_MYSQL_URL`.
 *
 *  The meta functions ask for an object's subtype (`get_object_subtype()`), which reads posts, comments, terms,
 *  users and sites, so their files and tables are loaded and seeded too:
 *
 *  - posts: 1 (`post`) and 2 (`page`);
 *  - comment 1, on post 1;
 *  - terms: 1 (`category`) and 2 (`post_tag`), with both taxonomies registered (`get_term()` drops a term whose
 *    taxonomy is not);
 *  - user 1;
 *  - on multisite, sites 1 and 2, and blog 2's own `wp_2_posts` (post 1) and `wp_2_postmeta`.
 *
 *  A custom meta type, `thing`, has its table `wp_thingmeta` (`$wpdb->thingmeta`, column `thing_id`) and no
 *  subtype.
 *
 *  `SPEEDYSPEC_COMPAT_MULTISITE` (false when undefined) makes the install a multisite one, as `wp-config.php` and
 *  `ms-settings.php` would: `MULTISITE`, network 1 with main site 1 as `$current_site`, and blog 1.
 */

if (!defined('SPEEDYSPEC_COMPAT_MULTISITE'))
{
    define('SPEEDYSPEC_COMPAT_MULTISITE', false);
}
if (SPEEDYSPEC_COMPAT_MULTISITE)
{
    foreach ([
        'MULTISITE' => true,
        'SUBDOMAIN_INSTALL' => false,
        'DOMAIN_CURRENT_SITE' => 'example.org',
        'PATH_CURRENT_SITE' => '/',
        'SITE_ID_CURRENT_SITE' => 1,
        'BLOG_ID_CURRENT_SITE' => 1,
    ] as $speedyspec_name => $speedyspec_value)
    {
        if (!defined($speedyspec_name))
        {
            define($speedyspec_name, $speedyspec_value);
        }
    }
}

$speedyspec_url = getenv('SPEEDYSPEC_TEST_MYSQL_URL');
if (!$speedyspec_url)
{
    throw new RuntimeException('the metadata compatibility scenarios need MySQL: set SPEEDYSPEC_TEST_MYSQL_URL');
}
$speedyspec_db = parse_url($speedyspec_url);

foreach ([
    'ABSPATH' => SPEEDYSPEC_COMPAT_WP_ROOT,
    'WPINC' => 'wp-includes',
    'WP_CONTENT_DIR' => SPEEDYSPEC_COMPAT_WP_ROOT . 'wp-content',
    'WP_DEBUG' => false,
    'WP_DEBUG_DISPLAY' => false,
    'DB_NAME' => ltrim($speedyspec_db['path'] ?? '', '/'),
    'DB_USER' => urldecode($speedyspec_db['user'] ?? ''),
    'DB_PASSWORD' => urldecode($speedyspec_db['pass'] ?? ''),
    'DB_HOST' => ($speedyspec_db['host'] ?? 'localhost') . (isset($speedyspec_db['port']) ? ':' . $speedyspec_db['port'] : ''),
    'DB_CHARSET' => 'utf8mb4',
    'DB_COLLATE' => '',
] as $speedyspec_name => $speedyspec_value)
{
    if (!defined($speedyspec_name))
    {
        define($speedyspec_name, $speedyspec_value);
    }
}

/**
 * Translates nothing: l10n.php is not loaded.
 */
function __($text, $domain = 'default')
{
    return $text;
}

/**
 * Translates nothing, with a context: l10n.php is not loaded.
 */
function _x($text, $context, $domain = 'default')
{
    return $text;
}

/**
 * Ends a scenario step where WordPress would end the page, as WordPress's test suite does.
 */
class WPDieException extends Exception
{
}

/**
 * Throws the message `wp_die()` was given, so a step reports it.
 */
function speedyspec_compat_wp_die($message, $title = '', $args = array())
{
    throw new WPDieException(is_string($message) ? $message : '');
}

/**
 * Reads an object's properties, which the driver reports only by class (a meta row from `get_metadata_by_mid()`,
 *  a `WP_Error`'s codes and messages).
 *
 * @return array<string, mixed>|mixed
 *   The object's accessible properties, or the value itself when it is not an object.
 */
function speedyspec_compat_props($value)
{
    return is_object($value) ? get_object_vars($value) : $value;
}

/**
 * Upper-cases a meta value (a `sanitize_callback` given by name).
 */
function speedyspec_compat_upper($value)
{
    return is_string($value) ? strtoupper($value) : $value;
}

/**
 * Answers a `get_{$meta_type}_metadata` filter with a list, so `$single` takes its first element.
 */
function speedyspec_compat_meta_list($value)
{
    return array('first', 'second');
}

/**
 * Answers a `get_{$meta_type}_metadata` filter with a string.
 */
function speedyspec_compat_meta_string($value)
{
    return 'short';
}

/**
 * Answers a `get_{$meta_type}_metadata` filter with an associative array, which `$single` also indexes by 0.
 */
function speedyspec_compat_meta_assoc($value)
{
    return array('a' => 'x', 0 => 'zero');
}

/**
 * Adds a meta value that is an object, which scenario JSON cannot carry.
 *
 * @return int|false
 *   What {@link add_metadata()} answers.
 */
function speedyspec_compat_add_object_meta($object_id, $meta_key)
{
    return add_metadata('post', $object_id, $meta_key, (object) ['a' => 1]);
}

/**
 * Caches an object, not an array, as a post's meta: an entry another process may have written, which 7.2 treats as
 *  a cache miss.
 */
function speedyspec_compat_object_meta_cache($object_id)
{
    return wp_cache_set($object_id, (object) ['k' => ['cached']], 'post_meta');
}

if (!SPEEDYSPEC_COMPAT_PORT)
{
    require ABSPATH . WPINC . '/plugin.php';
}
require ABSPATH . WPINC . '/load.php';
require ABSPATH . WPINC . '/compat.php';
require ABSPATH . WPINC . '/class-wp-error.php';
require ABSPATH . WPINC . '/formatting.php';
require ABSPATH . WPINC . '/functions.php';
require ABSPATH . WPINC . '/kses.php';
// meta.php requires class-wp-metadata-lazyloader.php itself.
require ABSPATH . WPINC . '/meta.php';
// What get_object_subtype() reads: posts, terms, comments, users (with their roles) and sites.
require ABSPATH . WPINC . '/class-wp-post.php';
require ABSPATH . WPINC . '/post.php';
require ABSPATH . WPINC . '/class-wp-term.php';
require ABSPATH . WPINC . '/class-wp-taxonomy.php';
require ABSPATH . WPINC . '/taxonomy.php';
require ABSPATH . WPINC . '/class-wp-comment.php';
require ABSPATH . WPINC . '/comment.php';
require ABSPATH . WPINC . '/class-wp-role.php';
require ABSPATH . WPINC . '/class-wp-roles.php';
require ABSPATH . WPINC . '/class-wp-user.php';
require ABSPATH . WPINC . '/capabilities.php';
require ABSPATH . WPINC . '/user.php';
require ABSPATH . WPINC . '/pluggable.php';
// register_meta() checks a registered default against its schema.
require ABSPATH . WPINC . '/rest-api.php';
if (SPEEDYSPEC_COMPAT_MULTISITE)
{
    require ABSPATH . WPINC . '/class-wp-network.php';
    require ABSPATH . WPINC . '/class-wp-site.php';
    require ABSPATH . WPINC . '/ms-network.php';
    require ABSPATH . WPINC . '/ms-site.php';
    require ABSPATH . WPINC . '/ms-blogs.php';
    // ms-settings.php: the current network and site, before the object cache reads the blog id.
    $GLOBALS['blog_id'] = 1;
    $GLOBALS['site_id'] = 1;
    $GLOBALS['current_site'] = new WP_Network((object) [
        'id' => 1,
        'domain' => 'example.org',
        'path' => '/',
        'blog_id' => 1,
        'site_name' => 'Compat Network',
        'cookie_domain' => 'example.org',
    ]);
}
if (!SPEEDYSPEC_COMPAT_PORT)
{
    require ABSPATH . WPINC . '/class-wp-object-cache.php';
    require ABSPATH . WPINC . '/cache.php';
    require ABSPATH . WPINC . '/class-wpdb.php';
    $GLOBALS['wpdb'] = new wpdb(DB_USER, DB_PASSWORD, DB_NAME, DB_HOST);
}
$GLOBALS['wpdb']->set_prefix('wp_');
if (SPEEDYSPEC_COMPAT_MULTISITE)
{
    // ms-settings.php: on multisite, set_prefix() leaves the blog tables to set_blog_id().
    $GLOBALS['wpdb']->set_blog_id(1, 1);
}
// The custom meta type's table, as a plugin declares it.
$GLOBALS['wpdb']->thingmeta = 'wp_thingmeta';
wp_cache_init();
// wp_start_object_cache()'s global groups.
wp_cache_add_global_groups(
    ['blog-details', 'blog-id-cache', 'blog-lookup', 'blog_meta', 'global-posts', 'image_editor', 'networks',
        'network-queries', 'sites', 'site-details', 'site-options', 'site-queries', 'site-transient', 'theme_files',
        'translation_files', 'rss', 'users', 'user-queries', 'user_meta', 'useremail', 'userlogins', 'userslugs']
);

// `init` has run, so `wp_load_translations_early()` (called by wpdb on invalid text) loads no l10n.php beside the
// stubs above.
do_action('init');
add_filter('wp_die_handler', static fn () => 'speedyspec_compat_wp_die');
// default-filters.php is not loaded; this is its one line the meta functions depend on.
add_filter('register_meta_args', '_wp_register_meta_args_allowed_list', 10, 2);
// create_initial_taxonomies()'s two the term seeds use, without rewrite rules.
register_taxonomy('category', 'post', ['hierarchical' => true, 'rewrite' => false, 'query_var' => false]);
register_taxonomy('post_tag', 'post', ['rewrite' => false, 'query_var' => false]);

/**
 * The meta tables each step reports, by meta type: the table and its id column.
 *
 * @return array<string, array{0: string, 1: string}>
 */
function speedyspec_compat_meta_tables(): array
{
    $tables = [
        'post' => ['wp_postmeta', 'meta_id'],
        'comment' => ['wp_commentmeta', 'meta_id'],
        'term' => ['wp_termmeta', 'meta_id'],
        'user' => ['wp_usermeta', 'umeta_id'],
        'thing' => ['wp_thingmeta', 'meta_id'],
    ];
    if (SPEEDYSPEC_COMPAT_MULTISITE)
    {
        $tables['blog'] = ['wp_blogmeta', 'meta_id'];
        $tables['post@2'] = ['wp_2_postmeta', 'meta_id'];
    }
    return $tables;
}

/**
 * Creates the tables if needed, empties them, and seeds the objects whose subtypes the scenarios read.
 */
function speedyspec_compat_meta_schema(): void
{
    global $wpdb;
    $charset = 'DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_520_ci';
    $meta = static fn (string $table, string $id, string $object): string =>
        "CREATE TABLE IF NOT EXISTS $table ($id bigint(20) unsigned NOT NULL auto_increment, "
        . "$object bigint(20) unsigned NOT NULL default '0', meta_key varchar(255) default NULL, meta_value longtext, "
        . "PRIMARY KEY ($id), KEY $object ($object), KEY meta_key (meta_key(191))) $charset";
    $posts = static fn (string $table): string =>
        "CREATE TABLE IF NOT EXISTS $table (ID bigint(20) unsigned NOT NULL auto_increment, "
        . "post_author bigint(20) unsigned NOT NULL default '0', "
        . "post_date datetime NOT NULL default '0000-00-00 00:00:00', "
        . "post_date_gmt datetime NOT NULL default '0000-00-00 00:00:00', post_content longtext NOT NULL, "
        . "post_title text NOT NULL, post_excerpt text NOT NULL, post_status varchar(20) NOT NULL default 'publish', "
        . "comment_status varchar(20) NOT NULL default 'open', ping_status varchar(20) NOT NULL default 'open', "
        . "post_password varchar(255) NOT NULL default '', post_name varchar(200) NOT NULL default '', "
        . "to_ping text NOT NULL, pinged text NOT NULL, "
        . "post_modified datetime NOT NULL default '0000-00-00 00:00:00', "
        . "post_modified_gmt datetime NOT NULL default '0000-00-00 00:00:00', post_content_filtered longtext NOT NULL, "
        . "post_parent bigint(20) unsigned NOT NULL default '0', guid varchar(255) NOT NULL default '', "
        . "menu_order int(11) NOT NULL default '0', post_type varchar(20) NOT NULL default 'post', "
        . "post_mime_type varchar(100) NOT NULL default '', comment_count bigint(20) NOT NULL default '0', "
        . "PRIMARY KEY (ID)) $charset";
    $post_row = static fn (int $id, string $type): array => [
        'ID' => $id, 'post_content' => '', 'post_title' => "Compat $id", 'post_excerpt' => '', 'to_ping' => '',
        'pinged' => '', 'post_content_filtered' => '', 'post_type' => $type, 'post_name' => "compat-$id",
        'post_date' => '2026-01-01 00:00:00', 'post_date_gmt' => '2026-01-01 00:00:00',
        'post_modified' => '2026-01-01 00:00:00', 'post_modified_gmt' => '2026-01-01 00:00:00',
    ];
    $wpdb->suppress_errors(true);
    $create = [
        $meta('wp_postmeta', 'meta_id', 'post_id'),
        $meta('wp_commentmeta', 'meta_id', 'comment_id'),
        $meta('wp_termmeta', 'meta_id', 'term_id'),
        $meta('wp_usermeta', 'umeta_id', 'user_id'),
        $meta('wp_thingmeta', 'meta_id', 'thing_id'),
        $posts('wp_posts'),
        "CREATE TABLE IF NOT EXISTS wp_comments (comment_ID bigint(20) unsigned NOT NULL auto_increment, "
        . "comment_post_ID bigint(20) unsigned NOT NULL default '0', comment_author tinytext NOT NULL, "
        . "comment_author_email varchar(100) NOT NULL default '', comment_author_url varchar(200) NOT NULL default '', "
        . "comment_author_IP varchar(100) NOT NULL default '', "
        . "comment_date datetime NOT NULL default '0000-00-00 00:00:00', "
        . "comment_date_gmt datetime NOT NULL default '0000-00-00 00:00:00', comment_content text NOT NULL, "
        . "comment_karma int(11) NOT NULL default '0', comment_approved varchar(20) NOT NULL default '1', "
        . "comment_agent varchar(255) NOT NULL default '', comment_type varchar(20) NOT NULL default 'comment', "
        . "comment_parent bigint(20) unsigned NOT NULL default '0', user_id bigint(20) unsigned NOT NULL default '0', "
        . "PRIMARY KEY (comment_ID)) $charset",
        "CREATE TABLE IF NOT EXISTS wp_terms (term_id bigint(20) unsigned NOT NULL auto_increment, "
        . "name varchar(200) NOT NULL default '', slug varchar(200) NOT NULL default '', "
        . "term_group bigint(10) NOT NULL default 0, PRIMARY KEY (term_id)) $charset",
        "CREATE TABLE IF NOT EXISTS wp_term_taxonomy (term_taxonomy_id bigint(20) unsigned NOT NULL auto_increment, "
        . "term_id bigint(20) unsigned NOT NULL default 0, taxonomy varchar(32) NOT NULL default '', "
        . "description longtext NOT NULL, parent bigint(20) unsigned NOT NULL default 0, "
        . "count bigint(20) NOT NULL default 0, PRIMARY KEY (term_taxonomy_id), "
        . "UNIQUE KEY term_id_taxonomy (term_id,taxonomy)) $charset",
        "CREATE TABLE IF NOT EXISTS wp_users (ID bigint(20) unsigned NOT NULL auto_increment, "
        . "user_login varchar(60) NOT NULL default '', user_pass varchar(255) NOT NULL default '', "
        . "user_nicename varchar(50) NOT NULL default '', user_email varchar(100) NOT NULL default '', "
        . "user_url varchar(100) NOT NULL default '', "
        . "user_registered datetime NOT NULL default '0000-00-00 00:00:00', "
        . "user_activation_key varchar(255) NOT NULL default '', user_status int(11) NOT NULL default '0', "
        . "display_name varchar(250) NOT NULL default '', PRIMARY KEY (ID)) $charset",
        // The same as compat/options' table: the two areas may share a database.
        "CREATE TABLE IF NOT EXISTS wp_options (option_id bigint(20) unsigned NOT NULL auto_increment, "
        . "option_name varchar(191) NOT NULL default '', option_value longtext NOT NULL, "
        . "autoload varchar(20) NOT NULL default 'yes', PRIMARY KEY (option_id), "
        . "UNIQUE KEY option_name (option_name), KEY autoload (autoload)) $charset",
    ];
    if (SPEEDYSPEC_COMPAT_MULTISITE)
    {
        $create[] = $meta('wp_blogmeta', 'meta_id', 'blog_id');
        $create[] = $meta('wp_2_postmeta', 'meta_id', 'post_id');
        $create[] = $posts('wp_2_posts');
        $create[] = "CREATE TABLE IF NOT EXISTS wp_blogs (blog_id bigint(20) unsigned NOT NULL auto_increment, "
            . "site_id bigint(20) unsigned NOT NULL default '0', domain varchar(200) NOT NULL default '', "
            . "path varchar(100) NOT NULL default '', "
            . "registered datetime NOT NULL default '0000-00-00 00:00:00', "
            . "last_updated datetime NOT NULL default '0000-00-00 00:00:00', public tinyint(2) NOT NULL default '1', "
            . "archived tinyint(2) NOT NULL default '0', mature tinyint(2) NOT NULL default '0', "
            . "spam tinyint(2) NOT NULL default '0', deleted tinyint(2) NOT NULL default '0', "
            . "lang_id int(11) NOT NULL default '0', PRIMARY KEY (blog_id)) $charset";
    }
    foreach ($create as $sql)
    {
        $wpdb->query($sql);
    }
    $tables = ['wp_posts', 'wp_comments', 'wp_terms', 'wp_term_taxonomy', 'wp_users', 'wp_options'];
    foreach (speedyspec_compat_meta_tables() as [$table, $_])
    {
        $tables[] = $table;
    }
    if (SPEEDYSPEC_COMPAT_MULTISITE)
    {
        $tables[] = 'wp_blogs';
        $tables[] = 'wp_2_posts';
    }
    foreach ($tables as $table)
    {
        $wpdb->query("TRUNCATE TABLE $table");
    }
    $wpdb->insert('wp_posts', $post_row(1, 'post'));
    $wpdb->insert('wp_posts', $post_row(2, 'page'));
    $wpdb->insert('wp_comments', [
        'comment_ID' => 1, 'comment_post_ID' => 1, 'comment_author' => 'Compat', 'comment_content' => 'Hello',
        'comment_date' => '2026-01-01 00:00:00', 'comment_date_gmt' => '2026-01-01 00:00:00',
    ]);
    $wpdb->insert('wp_terms', ['term_id' => 1, 'name' => 'Uncategorized', 'slug' => 'uncategorized']);
    $wpdb->insert('wp_terms', ['term_id' => 2, 'name' => 'Tagged', 'slug' => 'tagged']);
    foreach ([1 => 'category', 2 => 'post_tag'] as $id => $taxonomy)
    {
        $wpdb->insert(
            'wp_term_taxonomy',
            ['term_taxonomy_id' => $id, 'term_id' => $id, 'taxonomy' => $taxonomy, 'description' => '']
        );
    }
    $wpdb->insert('wp_users', [
        'ID' => 1, 'user_login' => 'compat', 'user_nicename' => 'compat', 'user_email' => 'compat@example.org',
        'user_registered' => '2026-01-01 00:00:00', 'display_name' => 'Compat',
    ]);
    if (SPEEDYSPEC_COMPAT_MULTISITE)
    {
        foreach ([1 => '/', 2 => '/two/'] as $id => $path)
        {
            $wpdb->insert('wp_blogs', [
                'blog_id' => $id, 'site_id' => 1, 'domain' => 'example.org', 'path' => $path,
                'registered' => '2026-01-01 00:00:00', 'last_updated' => '2026-01-01 00:00:00',
            ]);
        }
        $wpdb->insert('wp_2_posts', $post_row(1, 'page'));
    }
    $wpdb->suppress_errors(false);
}

/**
 * Prepares each scenario once the driver is loaded: emptied and seeded tables, and the state each step reports.
 */
function speedyspec_compat_area_setup(): void
{
    global $wpdb;
    speedyspec_compat_meta_schema();
    wp_cache_flush();
    SpeedySpecCompat::$report_globals = false;
    // A key or value that is not UTF-8 (the utf8mb4 scenarios) is reported as its exact bytes.
    SpeedySpecCompat::$exact_bytes = true;
    $last = $wpdb->num_queries;
    SpeedySpecCompat::$state = static function () use (&$last): array
    {
        global $wpdb, $wp_object_cache;
        $state = ['queries' => $wpdb->num_queries - $last];
        // Each meta table with rows, and each `{type}_meta` cache group with entries (on multisite a blog group's
        // keys carry the blog id).
        foreach (speedyspec_compat_meta_tables() as $type => [$table, $id])
        {
            $rows = $wpdb->get_results("SELECT * FROM $table ORDER BY $id", ARRAY_A);
            if ($rows)
            {
                $state["rows:$type"] = $rows;
            }
        }
        foreach (['post', 'comment', 'term', 'user', 'thing', 'blog'] as $type)
        {
            if (!empty($wp_object_cache->cache["{$type}_meta"]))
            {
                $state["cache:{$type}_meta"] = $wp_object_cache->cache["{$type}_meta"];
            }
        }
        if (SPEEDYSPEC_COMPAT_MULTISITE)
        {
            $state['blog_id'] = get_current_blog_id();
        }
        $last = $wpdb->num_queries;
        return $state;
    };
}
