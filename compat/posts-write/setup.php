<?php

declare(strict_types=1);

/**
 * Sets up the post-writing scenarios the same way on both sides: WordPress's own files, the same tables and seeds,
 *  the default callbacks the writers run, and the state each step reports.
 *
 *  `SPEEDYSPEC_COMPAT_WP_ROOT` is the release to load from and `SPEEDYSPEC_COMPAT_PORT` whether the native port is
 *  loaded. The recorder loads WordPress's hooks, object cache and `wpdb`, and the post, meta, comment, taxonomy,
 *  user, cron, rewrite, link and theme files the writers call; the port's side loads none of the first three (they
 *  are native) and its release copies of the others have the ported functions removed. Both need MySQL, through
 *  `SPEEDYSPEC_TEST_MYSQL_URL`, on a database of the area's own (`<database>_posts_write`): the seed empties every
 *  table it creates.
 *
 *  The site: `http://example.org`, plain permalinks, `gmt_offset` 2 (no `timezone_string`), a static front page
 *  (page 3), pingbacks on, no theme installed (`stylesheet` names a missing one), with `theme_page_templates`
 *  offering `tpl-wide.php` (`Wide`) as a theme's template header would. `sticky_posts` holds post 1.
 *
 *  The seed (blog 1), by ID: type, `post_name`, status, parent.
 *
 *  - 1 post `hello-world` publish, in category 1 and tag 2, meta `k` `v`, comments 1 (approved) and 2 (a reply to
 *    1, unapproved), each with meta; 4 its revision `1-revision-v1`; 5 attachment `photo` inherit (1, image/jpeg);
 *  - 2 page `parent-page` publish; 3 page `child-page` publish (2), the front page;
 *  - 6 post `taken` publish; 7 post `trashed-one__trashed` trash, its trash meta (status `publish`, desired slug
 *    `trashed-one`, comment 3's status `1`), comment 3 `post-trashed`; 8 post `draft-one` draft by user 2 with zero
 *    GMT dates; 21 post `trashed-one` publish, which took 7's slug while 7 was in the trash;
 *  - 9 page `loop-a` (10) and 10 page `loop-b` (9), a loop already stored; 11 page `cycle-a` and 12 page `cycle-b`
 *    (11); 13 page `leaf`;
 *  - 14 attachment `unattached` inherit (0, image/png); 15 post `future-one` future (2027-06-01 08:00 GMT);
 *  - 16 post auto-draft dated 2020 and 17 post auto-draft dated 2099 (MySQL's `NOW()` is not frozen, so the cleanup
 *    is chosen so the real date cannot change which one goes);
 *  - 18 post `private-one` private; 19 post `pending-one` pending; 20 page `trashed-page__trashed` trash (2), its
 *    trash meta (status `draft`, desired slug `trashed-page`); 22 post `old-trash` trash, trashed before WordPress
 *    added the suffix;
 *  - 23 post `no-guid` draft, 24 page `no-guid-page` draft and 25 `memo` `no-guid-memo` draft, each with an empty
 *    GUID, so publishing them runs `_transition_post_status()`'s GUID reset.
 *
 *  Users: 1 `compat` (administrator, with `unfiltered_html`) and 2 `writer` (author), with the roles in
 *  `wp_user_roles`. Terms: 1 `uncategorized` (category), 2 `tagged` (post_tag), 3 `news` (category).
 *
 *  `SPEEDYSPEC_COMPAT_MULTISITE` (false when undefined) makes the install a multisite one, as `wp-config.php` and
 *  `ms-settings.php` would: `MULTISITE`, network 1 with main site 1 as `$current_site`, blog 1, and `wp_sitemeta`
 *  (empty, so nobody is a super admin and `unfiltered_html` is denied). Blog 2 (`/two/`) has its own tables: posts
 *  (1 page `blog-two-home`, 2 post `blog-two-post`), meta, comments, options (`http://example.org/two`) and terms
 *  (1 `uncategorized`); both users are administrators there.
 *
 *  The default callbacks are default-filters.php's that write posts, meta, comments or the cron schedule, or that the
 *  writers' values pass through, in its order: the key, mime type, GUID and title sanitizers, kses (`kses_init` on
 *  `init` and `set_current_user`), `wp_check_post_hierarchy_for_loops`, the posts `last_changed` actions,
 *  `check_and_publish_future_post`, the revision savers, `_publish_post_hook`, `_transition_post_status`,
 *  `_update_term_count_on_transition_post_status`, `wp_check_for_changed_slugs` and `wp_check_for_changed_dates`,
 *  `delete_get_calendar_cache`, `__clear_multi_author_cache`, the front-page and privacy-page resets, and, on
 *  multisite, `wp_switch_roles_and_user`. Left out: the customizer's, nav menus', rewrite flushing and ping sending,
 *  and ms-default-filters.php's blog-details updates, which write none of the tables the steps snapshot.
 *
 *  Each step reports the queries it ran, the keys cached in the `posts`, `post_meta` and `counts` groups, and every
 *  value kept with `as` that changed (objects as their class and properties, since the driver reports an object by
 *  its class alone). The scenarios snapshot the tables themselves.
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
    throw new RuntimeException('the post-writing compatibility scenarios need MySQL: set SPEEDYSPEC_TEST_MYSQL_URL');
}
$speedyspec_db = parse_url($speedyspec_url);

foreach ([
    'ABSPATH' => SPEEDYSPEC_COMPAT_WP_ROOT,
    'WPINC' => 'wp-includes',
    'WP_CONTENT_DIR' => SPEEDYSPEC_COMPAT_WP_ROOT . 'wp-content',
    'WP_CONTENT_URL' => 'http://example.org/wp-content',
    'WP_DEBUG' => false,
    'WP_DEBUG_DISPLAY' => false,
    'DB_NAME' => ltrim($speedyspec_db['path'] ?? '', '/'),
    'DB_USER' => urldecode($speedyspec_db['user'] ?? ''),
    'DB_PASSWORD' => urldecode($speedyspec_db['pass'] ?? ''),
    'DB_HOST' => ($speedyspec_db['host'] ?? 'localhost')
        . (isset($speedyspec_db['port']) ? ':' . $speedyspec_db['port'] : ''),
    'DB_CHARSET' => 'utf8mb4',
    'DB_COLLATE' => '',
    // wp_initial_constants()'s time constants and wp_functionality_constants()' (default-constants.php is not
    // loaded).
    'MINUTE_IN_SECONDS' => 60,
    'HOUR_IN_SECONDS' => 3600,
    'DAY_IN_SECONDS' => 86400,
    'WEEK_IN_SECONDS' => 604800,
    'MONTH_IN_SECONDS' => 2592000,
    'YEAR_IN_SECONDS' => 31536000,
    'KB_IN_BYTES' => 1024,
    'MB_IN_BYTES' => 1048576,
    'AUTOSAVE_INTERVAL' => 60,
    'EMPTY_TRASH_DAYS' => 30,
    'MEDIA_TRASH' => false,
    'WP_POST_REVISIONS' => true,
    'WP_CRON_LOCK_TIMEOUT' => 60,
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
 * Picks the singular or plural without translating: l10n.php is not loaded.
 */
function _n($single, $plural, $number, $domain = 'default')
{
    return 1 === (int) $number ? $single : $plural;
}

/**
 * Escapes for HTML without translating: l10n.php is not loaded.
 */
function esc_html__($text, $domain = 'default')
{
    return esc_html($text);
}

/**
 * Escapes for an attribute without translating: l10n.php is not loaded.
 */
function esc_attr__($text, $domain = 'default')
{
    return esc_attr($text);
}

/**
 * Registers plural strings without translating them, as l10n.php's own does (the post statuses' `label_count`).
 *
 * @return array<int|string, string|null>
 */
function _n_noop($singular, $plural, $domain = null)
{
    return array(
        0 => $singular,
        1 => $plural,
        'singular' => $singular,
        'plural' => $plural,
        'context' => null,
        'domain' => $domain,
    );
}

/**
 * Answers the site's locale: l10n.php is not loaded.
 */
function get_locale()
{
    return 'en_US';
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
 * A value as plain data the driver can report: an object becomes its class and properties (a fixture callback stays
 *  its label).
 *
 * @return mixed
 */
function speedyspec_compat_plain($value, int $depth = 0)
{
    if ($depth > 8)
    {
        return is_object($value) ? ['class' => get_class($value)] : $value;
    }
    if (is_array($value))
    {
        $out = [];
        foreach ($value as $key => $item)
        {
            $out[$key] = speedyspec_compat_plain($item, $depth + 1);
        }
        return $out;
    }
    if (is_object($value))
    {
        $label = SpeedySpecCompat::$labels[spl_object_id($value)] ?? null;
        if ($label !== null && !str_starts_with($label, '$'))
        {
            return ['callback' => $label];
        }
        if ($value instanceof Closure)
        {
            return ['class' => 'Closure'];
        }
        return [
            'class' => get_class($value),
            'props' => speedyspec_compat_plain(get_object_vars($value), $depth + 1),
        ];
    }
    return $value;
}

/**
 * The page templates a theme's template headers would offer pages: `tpl-wide.php`, named `Wide`.
 *
 * @param array<string, string> $templates
 * @return array<string, string>
 */
function speedyspec_compat_posts_write_page_templates($templates): array
{
    return array_merge((array) $templates, ['tpl-wide.php' => 'Wide']);
}

/**
 * Answers `pre_wp_unique_post_slug` with a slug of its own, so wp_unique_post_slug() checks nothing.
 */
function speedyspec_compat_posts_write_forced_slug($override, $slug): string
{
    return 'forced-' . $slug;
}

/**
 * Answers `wp_insert_post_empty_content` with true, so wp_insert_post() refuses the post as empty.
 */
function speedyspec_compat_posts_write_always_empty($maybe_empty): bool
{
    return true;
}

/**
 * Registers the types and taxonomy the scenarios add to the core ones.
 *
 *  The `genre` taxonomy is the one tax_input names, `book` a hierarchical type, and `memo` a public type that is
 *  not publicly queryable, whose permalink (and so its reset GUID) takes the non-viewable form.
 */
function speedyspec_compat_posts_write_register(): bool
{
    register_taxonomy('genre', 'post', ['hierarchical' => false, 'rewrite' => false, 'query_var' => false]);
    register_post_type('book', ['public' => true, 'hierarchical' => true, 'rewrite' => false, 'query_var' => false]);
    register_post_type('memo', [
        'public' => true,
        'publicly_queryable' => false,
        'rewrite' => false,
        'query_var' => false,
    ]);
    return true;
}

if (!SPEEDYSPEC_COMPAT_PORT)
{
    require ABSPATH . WPINC . '/plugin.php';
}
require ABSPATH . WPINC . '/load.php';
// The port's release copies stub these two out (speedyspec-wp-utf8-php), as wp-settings.php orders them.
require ABSPATH . WPINC . '/compat-utf8.php';
require ABSPATH . WPINC . '/compat.php';
require ABSPATH . WPINC . '/class-wp-list-util.php';
require ABSPATH . WPINC . '/class-wp-token-map.php';
require ABSPATH . WPINC . '/utf8.php';
require ABSPATH . WPINC . '/class-wp-error.php';
require ABSPATH . WPINC . '/formatting.php';
require ABSPATH . WPINC . '/meta.php';
require ABSPATH . WPINC . '/functions.php';
require ABSPATH . WPINC . '/class-wp-meta-query.php';
require ABSPATH . WPINC . '/class-wp-matchesmapregex.php';
require ABSPATH . WPINC . '/kses.php';
// The HTML API kses uses on content a user without `unfiltered_html` saves.
require ABSPATH . WPINC . '/html-api/html5-named-character-references.php';
require ABSPATH . WPINC . '/html-api/class-wp-html-attribute-token.php';
require ABSPATH . WPINC . '/html-api/class-wp-html-span.php';
require ABSPATH . WPINC . '/html-api/class-wp-html-doctype-info.php';
require ABSPATH . WPINC . '/html-api/class-wp-html-text-replacement.php';
require ABSPATH . WPINC . '/html-api/class-wp-html-decoder.php';
require ABSPATH . WPINC . '/html-api/class-wp-html-tag-processor.php';
require ABSPATH . WPINC . '/html-api/class-wp-html-unsupported-exception.php';
require ABSPATH . WPINC . '/html-api/class-wp-html-active-formatting-elements.php';
require ABSPATH . WPINC . '/html-api/class-wp-html-open-elements.php';
require ABSPATH . WPINC . '/html-api/class-wp-html-token.php';
require ABSPATH . WPINC . '/html-api/class-wp-html-stack-event.php';
require ABSPATH . WPINC . '/html-api/class-wp-html-processor-state.php';
require ABSPATH . WPINC . '/html-api/class-wp-html-processor.php';
require ABSPATH . WPINC . '/capabilities.php';
require ABSPATH . WPINC . '/class-wp-roles.php';
require ABSPATH . WPINC . '/class-wp-role.php';
require ABSPATH . WPINC . '/class-wp-user.php';
// The revisions wp_save_post_revision() looks up through get_children().
require ABSPATH . WPINC . '/class-wp-query.php';
require ABSPATH . WPINC . '/query.php';
require ABSPATH . WPINC . '/class-wp-date-query.php';
// wp_get_theme()'s page templates.
require ABSPATH . WPINC . '/theme.php';
require ABSPATH . WPINC . '/class-wp-theme.php';
require ABSPATH . WPINC . '/user.php';
require ABSPATH . WPINC . '/general-template.php';
// get_permalink() for the GUID.
require ABSPATH . WPINC . '/link-template.php';
require ABSPATH . WPINC . '/author-template.php';
require ABSPATH . WPINC . '/post.php';
require ABSPATH . WPINC . '/class-wp-post-type.php';
require ABSPATH . WPINC . '/class-wp-post.php';
require ABSPATH . WPINC . '/post-template.php';
require ABSPATH . WPINC . '/revision.php';
require ABSPATH . WPINC . '/category.php';
require ABSPATH . WPINC . '/category-template.php';
require ABSPATH . WPINC . '/comment.php';
require ABSPATH . WPINC . '/class-wp-comment.php';
require ABSPATH . WPINC . '/class-wp-comment-query.php';
require ABSPATH . WPINC . '/rewrite.php';
require ABSPATH . WPINC . '/class-wp-rewrite.php';
require ABSPATH . WPINC . '/cron.php';
require ABSPATH . WPINC . '/taxonomy.php';
require ABSPATH . WPINC . '/class-wp-taxonomy.php';
require ABSPATH . WPINC . '/class-wp-term.php';
require ABSPATH . WPINC . '/class-wp-term-query.php';
require ABSPATH . WPINC . '/class-wp-tax-query.php';
require ABSPATH . WPINC . '/pluggable.php';
if (SPEEDYSPEC_COMPAT_MULTISITE)
{
    require ABSPATH . WPINC . '/class-wp-network.php';
    require ABSPATH . WPINC . '/class-wp-site.php';
    require ABSPATH . WPINC . '/ms-network.php';
    require ABSPATH . WPINC . '/ms-site.php';
    require ABSPATH . WPINC . '/ms-blogs.php';
    require ABSPATH . WPINC . '/ms-functions.php';
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
// wp_cache_get_salted() and friends, as wp-settings.php loads them.
require ABSPATH . WPINC . '/cache-compat.php';
$GLOBALS['wpdb']->set_prefix('wp_');
if (SPEEDYSPEC_COMPAT_MULTISITE)
{
    // ms-settings.php: on multisite, set_prefix() leaves the blog tables to set_blog_id().
    $GLOBALS['wpdb']->set_blog_id(1, 1);
}
$GLOBALS['table_prefix'] = 'wp_';
wp_cache_init();
// wp_start_object_cache()'s global groups.
wp_cache_add_global_groups(
    ['blog-details', 'blog-id-cache', 'blog-lookup', 'blog_meta', 'global-posts', 'image_editor', 'networks',
        'network-queries', 'sites', 'site-details', 'site-options', 'site-queries', 'site-transient', 'theme_files',
        'translation_files', 'rss', 'users', 'user-queries', 'user_meta', 'useremail', 'userlogins', 'userslugs']
);

/**
 * The tables the seed creates and empties, by name: their `CREATE TABLE` statements (wp-admin/includes/schema.php's).
 *
 * @return array<string, string>
 */
function speedyspec_compat_posts_write_tables(): array
{
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
        . "PRIMARY KEY (ID), KEY post_name (post_name(191)), "
        . "KEY type_status_date (post_type,post_status,post_date,ID), KEY post_parent (post_parent), "
        . "KEY post_author (post_author)) $charset";
    $comments = static fn (string $table): string =>
        "CREATE TABLE IF NOT EXISTS $table (comment_ID bigint(20) unsigned NOT NULL auto_increment, "
        . "comment_post_ID bigint(20) unsigned NOT NULL default '0', comment_author tinytext NOT NULL, "
        . "comment_author_email varchar(100) NOT NULL default '', comment_author_url varchar(200) NOT NULL default '', "
        . "comment_author_IP varchar(100) NOT NULL default '', "
        . "comment_date datetime NOT NULL default '0000-00-00 00:00:00', "
        . "comment_date_gmt datetime NOT NULL default '0000-00-00 00:00:00', comment_content text NOT NULL, "
        . "comment_karma int(11) NOT NULL default '0', comment_approved varchar(20) NOT NULL default '1', "
        . "comment_agent varchar(255) NOT NULL default '', comment_type varchar(20) NOT NULL default 'comment', "
        . "comment_parent bigint(20) unsigned NOT NULL default '0', user_id bigint(20) unsigned NOT NULL default '0', "
        . "PRIMARY KEY (comment_ID), KEY comment_post_ID (comment_post_ID), "
        . "KEY comment_approved_date_gmt (comment_approved,comment_date_gmt), KEY comment_parent (comment_parent)) "
        . "$charset";
    $options = static fn (string $table): string =>
        "CREATE TABLE IF NOT EXISTS $table (option_id bigint(20) unsigned NOT NULL auto_increment, "
        . "option_name varchar(191) NOT NULL default '', option_value longtext NOT NULL, "
        . "autoload varchar(20) NOT NULL default 'yes', PRIMARY KEY (option_id), "
        . "UNIQUE KEY option_name (option_name), KEY autoload (autoload)) $charset";
    $terms = static fn (string $prefix): array => [
        "{$prefix}terms" => "CREATE TABLE IF NOT EXISTS {$prefix}terms "
            . "(term_id bigint(20) unsigned NOT NULL auto_increment, name varchar(200) NOT NULL default '', "
            . "slug varchar(200) NOT NULL default '', term_group bigint(10) NOT NULL default 0, "
            . "PRIMARY KEY (term_id)) $charset",
        "{$prefix}term_taxonomy" => "CREATE TABLE IF NOT EXISTS {$prefix}term_taxonomy "
            . "(term_taxonomy_id bigint(20) unsigned NOT NULL auto_increment, "
            . "term_id bigint(20) unsigned NOT NULL default 0, taxonomy varchar(32) NOT NULL default '', "
            . "description longtext NOT NULL, parent bigint(20) unsigned NOT NULL default 0, "
            . "count bigint(20) NOT NULL default 0, PRIMARY KEY (term_taxonomy_id), "
            . "UNIQUE KEY term_id_taxonomy (term_id,taxonomy)) $charset",
        "{$prefix}term_relationships" => "CREATE TABLE IF NOT EXISTS {$prefix}term_relationships "
            . "(object_id bigint(20) unsigned NOT NULL default 0, "
            . "term_taxonomy_id bigint(20) unsigned NOT NULL default 0, term_order int(11) NOT NULL default 0, "
            . "PRIMARY KEY (object_id,term_taxonomy_id)) $charset",
        "{$prefix}termmeta" => $meta("{$prefix}termmeta", 'meta_id', 'term_id'),
    ];
    $blog = static fn (string $prefix): array => array_merge([
        "{$prefix}posts" => $posts("{$prefix}posts"),
        "{$prefix}postmeta" => $meta("{$prefix}postmeta", 'meta_id', 'post_id'),
        "{$prefix}comments" => $comments("{$prefix}comments"),
        "{$prefix}commentmeta" => $meta("{$prefix}commentmeta", 'meta_id', 'comment_id'),
        "{$prefix}options" => $options("{$prefix}options"),
    ], $terms($prefix));
    $tables = array_merge($blog('wp_'), [
        'wp_usermeta' => $meta('wp_usermeta', 'umeta_id', 'user_id'),
        'wp_users' => "CREATE TABLE IF NOT EXISTS wp_users (ID bigint(20) unsigned NOT NULL auto_increment, "
            . "user_login varchar(60) NOT NULL default '', user_pass varchar(255) NOT NULL default '', "
            . "user_nicename varchar(50) NOT NULL default '', user_email varchar(100) NOT NULL default '', "
            . "user_url varchar(100) NOT NULL default '', "
            . "user_registered datetime NOT NULL default '0000-00-00 00:00:00', "
            . "user_activation_key varchar(255) NOT NULL default '', user_status int(11) NOT NULL default '0', "
            . "display_name varchar(250) NOT NULL default '', PRIMARY KEY (ID)) $charset",
    ]);
    if (SPEEDYSPEC_COMPAT_MULTISITE)
    {
        $tables = array_merge($tables, $blog('wp_2_'), [
            'wp_blogs' => "CREATE TABLE IF NOT EXISTS wp_blogs (blog_id bigint(20) unsigned NOT NULL auto_increment, "
                . "site_id bigint(20) unsigned NOT NULL default '0', domain varchar(200) NOT NULL default '', "
                . "path varchar(100) NOT NULL default '', "
                . "registered datetime NOT NULL default '0000-00-00 00:00:00', "
                . "last_updated datetime NOT NULL default '0000-00-00 00:00:00', "
                . "public tinyint(2) NOT NULL default '1', archived tinyint(2) NOT NULL default '0', "
                . "mature tinyint(2) NOT NULL default '0', spam tinyint(2) NOT NULL default '0', "
                . "deleted tinyint(2) NOT NULL default '0', lang_id int(11) NOT NULL default '0', "
                . "PRIMARY KEY (blog_id)) $charset",
            'wp_sitemeta' => "CREATE TABLE IF NOT EXISTS wp_sitemeta "
                . "(meta_id bigint(20) unsigned NOT NULL auto_increment, "
                . "site_id bigint(20) unsigned NOT NULL default '0', meta_key varchar(255) default NULL, "
                . "meta_value longtext, PRIMARY KEY (meta_id), KEY meta_key (meta_key(191)), KEY site_id (site_id)) "
                . "$charset",
        ]);
    }
    return $tables;
}

/**
 * A seeded post row: the type, name, status and parent, with the rest defaulted from the ID.
 *
 * @return array<string, int|string>
 */
function speedyspec_compat_posts_write_row(
    int $id,
    string $type,
    string $name,
    string $status,
    int $parent,
    array $more = []
): array
{
    $day = sprintf('%02d', $id);
    return array_merge([
        'ID' => $id,
        'post_author' => 1,
        'post_date' => "2026-01-$day 10:00:00",
        'post_date_gmt' => "2026-01-$day 08:00:00",
        'post_content' => "Content $id",
        'post_title' => "Title $id",
        'post_excerpt' => '',
        'post_status' => $status,
        'post_name' => $name,
        'to_ping' => '',
        'pinged' => '',
        'post_modified' => "2026-02-$day 10:00:00",
        'post_modified_gmt' => "2026-02-$day 08:00:00",
        'post_content_filtered' => '',
        'post_parent' => $parent,
        'guid' => "http://example.org/?p=$id",
        'post_type' => $type,
    ], $more);
}

/**
 * The roles, as `wp_user_roles` stores them.
 *
 * @return array<string, array{name: string, capabilities: array<string, bool>}>
 */
function speedyspec_compat_posts_write_roles(): array
{
    $author = ['read', 'upload_files', 'edit_posts', 'edit_published_posts', 'publish_posts', 'delete_posts',
        'delete_published_posts'];
    $admin = array_merge($author, ['edit_others_posts', 'delete_others_posts', 'delete_private_posts',
        'edit_private_posts', 'read_private_posts', 'edit_pages', 'edit_others_pages', 'edit_published_pages',
        'publish_pages', 'delete_pages', 'delete_others_pages', 'delete_published_pages', 'delete_private_pages',
        'edit_private_pages', 'read_private_pages', 'manage_categories', 'unfiltered_html', 'manage_options']);
    return [
        'administrator' => ['name' => 'Administrator', 'capabilities' => array_fill_keys($admin, true)],
        'author' => ['name' => 'Author', 'capabilities' => array_fill_keys($author, true)],
    ];
}

/**
 * A blog's options: its address, the writing defaults, the roles and the missing theme.
 *
 * @return array<string, string>
 */
function speedyspec_compat_posts_write_options(string $prefix, string $home, array $more = []): array
{
    return array_merge([
        'siteurl' => $home,
        'home' => $home,
        'blogname' => 'Compat',
        'blog_charset' => 'UTF-8',
        'permalink_structure' => '',
        'gmt_offset' => '2',
        'timezone_string' => '',
        'default_category' => '1',
        'default_comment_status' => 'open',
        'default_ping_status' => 'open',
        'default_pingback_flag' => '1',
        'show_on_front' => 'posts',
        'page_on_front' => '0',
        'page_for_posts' => '0',
        'wp_page_for_privacy_policy' => '0',
        'sticky_posts' => serialize([]),
        'stylesheet' => 'speedyspec-compat-none',
        'template' => 'speedyspec-compat-none',
        'upload_path' => '',
        'uploads_use_yearmonth_folders' => '1',
        "{$prefix}user_roles" => serialize(speedyspec_compat_posts_write_roles()),
    ], $more);
}

/**
 * Creates the tables if needed, empties them, and seeds the site the scenarios write to.
 */
function speedyspec_compat_posts_write_schema(): void
{
    global $wpdb;
    $wpdb->suppress_errors(true);
    // The tables a step snapshots start their IDs again from 1 (wp_term_relationships has none); the rest are
    // emptied in the seed's one transaction, which keeps seeding quick on a disk that syncs every commit, and their
    // seeded rows carry explicit IDs.
    $renumbered = '/^wp_(2_)?(posts|postmeta|comments|commentmeta)$/';
    foreach (speedyspec_compat_posts_write_tables() as $table => $create)
    {
        $wpdb->query($create);
        if (preg_match($renumbered, $table))
        {
            $wpdb->query("TRUNCATE TABLE $table");
        }
    }
    $wpdb->query('START TRANSACTION');
    foreach (array_keys(speedyspec_compat_posts_write_tables()) as $table)
    {
        if (!preg_match($renumbered, $table))
        {
            $wpdb->query("DELETE FROM $table");
        }
    }
    $zero = '0000-00-00 00:00:00';
    $rows = [
        speedyspec_compat_posts_write_row(1, 'post', 'hello-world', 'publish', 0, ['comment_count' => 1]),
        speedyspec_compat_posts_write_row(2, 'page', 'parent-page', 'publish', 0, [
            'guid' => 'http://example.org/?page_id=2',
        ]),
        speedyspec_compat_posts_write_row(3, 'page', 'child-page', 'publish', 2, [
            'guid' => 'http://example.org/?page_id=3',
        ]),
        speedyspec_compat_posts_write_row(4, 'revision', '1-revision-v1', 'inherit', 1),
        speedyspec_compat_posts_write_row(5, 'attachment', 'photo', 'inherit', 1, ['post_mime_type' => 'image/jpeg']),
        speedyspec_compat_posts_write_row(6, 'post', 'taken', 'publish', 0),
        speedyspec_compat_posts_write_row(7, 'post', 'trashed-one__trashed', 'trash', 0),
        speedyspec_compat_posts_write_row(8, 'post', '', 'draft', 0, [
            'post_author' => 2,
            'post_date_gmt' => $zero,
            'post_modified_gmt' => $zero,
        ]),
        speedyspec_compat_posts_write_row(9, 'page', 'loop-a', 'publish', 10),
        speedyspec_compat_posts_write_row(10, 'page', 'loop-b', 'publish', 9),
        speedyspec_compat_posts_write_row(11, 'page', 'cycle-a', 'publish', 0),
        speedyspec_compat_posts_write_row(12, 'page', 'cycle-b', 'publish', 11),
        speedyspec_compat_posts_write_row(13, 'page', 'leaf', 'publish', 0),
        speedyspec_compat_posts_write_row(14, 'attachment', 'unattached', 'inherit', 0, [
            'post_mime_type' => 'image/png',
        ]),
        speedyspec_compat_posts_write_row(15, 'post', 'future-one', 'future', 0, [
            'post_date' => '2027-06-01 10:00:00',
            'post_date_gmt' => '2027-06-01 08:00:00',
        ]),
        speedyspec_compat_posts_write_row(16, 'post', '', 'auto-draft', 0, [
            'post_title' => 'Auto Draft',
            'post_date' => '2020-01-01 10:00:00',
            'post_date_gmt' => $zero,
        ]),
        speedyspec_compat_posts_write_row(17, 'post', '', 'auto-draft', 0, [
            'post_title' => 'Auto Draft',
            'post_date' => '2099-01-01 10:00:00',
            'post_date_gmt' => $zero,
        ]),
        speedyspec_compat_posts_write_row(18, 'post', 'private-one', 'private', 0),
        speedyspec_compat_posts_write_row(19, 'post', 'pending-one', 'pending', 0, ['post_date_gmt' => $zero]),
        speedyspec_compat_posts_write_row(20, 'page', 'trashed-page__trashed', 'trash', 2),
        speedyspec_compat_posts_write_row(21, 'post', 'trashed-one', 'publish', 0),
        speedyspec_compat_posts_write_row(22, 'post', 'old-trash', 'trash', 0),
        speedyspec_compat_posts_write_row(23, 'post', 'no-guid', 'draft', 0, ['guid' => '']),
        speedyspec_compat_posts_write_row(24, 'page', 'no-guid-page', 'draft', 0, ['guid' => '']),
        speedyspec_compat_posts_write_row(25, 'memo', 'no-guid-memo', 'draft', 0, ['guid' => '']),
    ];
    foreach ($rows as $row)
    {
        $wpdb->insert('wp_posts', $row);
    }
    $meta = [
        [1, 'k', 'v'],
        [5, '_wp_attached_file', '2026/01/photo.jpg'],
        [7, '_wp_trash_meta_status', 'publish'],
        [7, '_wp_trash_meta_time', '1795000000'],
        [7, '_wp_desired_post_slug', 'trashed-one'],
        [7, '_wp_trash_meta_comments_status', serialize([3 => '1'])],
        [20, '_wp_trash_meta_status', 'draft'],
        [20, '_wp_trash_meta_time', '1795000000'],
        [20, '_wp_desired_post_slug', 'trashed-page'],
    ];
    foreach ($meta as [$id, $key, $value])
    {
        $wpdb->insert('wp_postmeta', ['post_id' => $id, 'meta_key' => $key, 'meta_value' => $value]);
    }
    $comments = [
        [1, 1, '1', 0],
        [2, 1, '0', 1],
        [3, 7, 'post-trashed', 0],
    ];
    foreach ($comments as [$id, $post, $approved, $parent])
    {
        $wpdb->insert('wp_comments', [
            'comment_ID' => $id, 'comment_post_ID' => $post, 'comment_author' => "Reader $id",
            'comment_author_email' => "reader$id@example.org", 'comment_date' => '2026-01-20 10:00:00',
            'comment_date_gmt' => '2026-01-20 08:00:00', 'comment_content' => "Comment $id",
            'comment_approved' => $approved, 'comment_parent' => $parent,
        ]);
    }
    $wpdb->insert('wp_commentmeta', ['meta_id' => 1, 'comment_id' => 1, 'meta_key' => 'rating', 'meta_value' => '5']);
    $wpdb->insert('wp_commentmeta', ['meta_id' => 2, 'comment_id' => 2, 'meta_key' => 'rating', 'meta_value' => '3']);
    foreach ([1 => ['compat', 'administrator'], 2 => ['writer', 'author']] as $id => [$login, $role])
    {
        $wpdb->insert('wp_users', [
            'ID' => $id, 'user_login' => $login, 'user_nicename' => $login, 'user_email' => "$login@example.org",
            'user_registered' => '2026-01-01 00:00:00', 'display_name' => ucfirst($login),
        ]);
        $wpdb->insert('wp_usermeta', [
            'umeta_id' => $id, 'user_id' => $id, 'meta_key' => 'wp_capabilities',
            'meta_value' => serialize([$role => true]),
        ]);
        if (SPEEDYSPEC_COMPAT_MULTISITE)
        {
            $wpdb->insert('wp_usermeta', [
                'umeta_id' => 10 + $id, 'user_id' => $id, 'meta_key' => 'wp_2_capabilities',
                'meta_value' => serialize(['administrator' => true]),
            ]);
        }
    }
    $options = speedyspec_compat_posts_write_options('wp_', 'http://example.org', [
        'show_on_front' => 'page',
        'page_on_front' => '3',
        'sticky_posts' => serialize([1]),
    ]);
    foreach ($options as $name => $value)
    {
        $wpdb->insert('wp_options', ['option_name' => $name, 'option_value' => $value, 'autoload' => 'on']);
    }
    $terms = [[1, 'Uncategorized', 'uncategorized', 'category', 1], [2, 'Tagged', 'tagged', 'post_tag', 1],
        [3, 'News', 'news', 'category', 0]];
    foreach ($terms as [$id, $name, $slug, $taxonomy, $count])
    {
        $wpdb->insert('wp_terms', ['term_id' => $id, 'name' => $name, 'slug' => $slug]);
        $wpdb->insert('wp_term_taxonomy', [
            'term_taxonomy_id' => $id, 'term_id' => $id, 'taxonomy' => $taxonomy, 'description' => '',
            'count' => $count,
        ]);
    }
    foreach ([1, 2] as $id)
    {
        $wpdb->insert('wp_term_relationships', ['object_id' => 1, 'term_taxonomy_id' => $id]);
    }
    if (SPEEDYSPEC_COMPAT_MULTISITE)
    {
        foreach ([1 => '/', 2 => '/two/'] as $id => $path)
        {
            $wpdb->insert('wp_blogs', [
                'blog_id' => $id, 'site_id' => 1, 'domain' => 'example.org', 'path' => $path,
                'registered' => '2026-01-01 00:00:00', 'last_updated' => '2026-01-01 00:00:00',
            ]);
        }
        foreach ([
            speedyspec_compat_posts_write_row(1, 'page', 'blog-two-home', 'publish', 0, [
                'guid' => 'http://example.org/two/?page_id=1',
            ]),
            speedyspec_compat_posts_write_row(2, 'post', 'blog-two-post', 'publish', 0, [
                'guid' => 'http://example.org/two/?p=2',
            ]),
        ] as $row)
        {
            $wpdb->insert('wp_2_posts', $row);
        }
        $wpdb->insert('wp_2_postmeta', ['post_id' => 2, 'meta_key' => 'k', 'meta_value' => 'blog-2']);
        foreach (speedyspec_compat_posts_write_options('wp_2_', 'http://example.org/two') as $name => $value)
        {
            $wpdb->insert('wp_2_options', ['option_name' => $name, 'option_value' => $value, 'autoload' => 'on']);
        }
        $wpdb->insert('wp_2_terms', ['term_id' => 1, 'name' => 'Uncategorized', 'slug' => 'uncategorized']);
        $wpdb->insert('wp_2_term_taxonomy', [
            'term_taxonomy_id' => 1, 'term_id' => 1, 'taxonomy' => 'category', 'description' => '', 'count' => 1,
        ]);
        $wpdb->insert('wp_2_term_relationships', ['object_id' => 2, 'term_taxonomy_id' => 1]);
    }
    $wpdb->query('COMMIT');
    $wpdb->suppress_errors(false);
}

// The seed runs before WordPress registers anything, as an installed site's tables exist before the request.
speedyspec_compat_posts_write_schema();

// wp-settings.php's rewrite object, which wp_unique_post_slug() and get_permalink() read.
$GLOBALS['wp_rewrite'] = new WP_Rewrite();

// default-filters.php's callbacks for the writers, in its order (see the file comment).
foreach (['pre_post_type', 'pre_post_status', 'pre_post_comment_status', 'pre_post_ping_status'] as $speedyspec_filter)
{
    add_filter($speedyspec_filter, 'sanitize_key');
}
foreach (['wp_strip_all_tags', 'sanitize_url', 'wp_filter_kses'] as $speedyspec_callback)
{
    add_filter('pre_post_guid', $speedyspec_callback);
}
add_filter('pre_post_mime_type', 'sanitize_mime_type');
add_filter('post_mime_type', 'sanitize_mime_type');
add_action('added_post_meta', 'wp_cache_set_posts_last_changed');
add_action('updated_post_meta', 'wp_cache_set_posts_last_changed');
add_action('deleted_post_meta', 'wp_cache_set_posts_last_changed');
add_action('added_comment_meta', 'wp_cache_set_comments_last_changed');
add_action('updated_comment_meta', 'wp_cache_set_comments_last_changed');
add_action('deleted_comment_meta', 'wp_cache_set_comments_last_changed');
add_filter('wp_insert_post_parent', 'wp_check_post_hierarchy_for_loops', 10, 2);
add_filter('title_save_pre', 'trim');
// The sanitize_title filter wp_unique_post_slug() and sanitize_title() rely on.
add_filter('sanitize_title', 'sanitize_title_with_dashes', 10, 3);
add_action('publish_future_post', 'check_and_publish_future_post', 10, 1);
add_action('wp_after_insert_post', 'wp_save_post_revision_on_insert', 9, 3);
add_action('post_updated', 'wp_save_post_revision', 10, 1);
add_action('publish_post', '_publish_post_hook', 5, 1);
add_action('transition_post_status', '_transition_post_status', 5, 3);
add_action('transition_post_status', '_update_term_count_on_transition_post_status', 10, 3);
add_action('post_updated', 'wp_check_for_changed_slugs', 12, 3);
add_action('attachment_updated', 'wp_check_for_changed_slugs', 12, 3);
add_action('post_updated', 'wp_check_for_changed_dates', 12, 3);
add_action('attachment_updated', 'wp_check_for_changed_dates', 12, 3);
add_action('save_post', 'delete_get_calendar_cache');
add_action('delete_post', 'delete_get_calendar_cache');
add_action('transition_post_status', '__clear_multi_author_cache');
add_action('before_delete_post', '_reset_front_page_settings_for_post');
add_action('before_delete_post', '_reset_privacy_policy_page_for_post');
add_action('wp_trash_post', '_reset_front_page_settings_for_post');
add_action('init', 'kses_init');
add_action('set_current_user', 'kses_init');
if (SPEEDYSPEC_COMPAT_MULTISITE)
{
    // ms-default-filters.php's: switching blogs switches the roles and the current user's capabilities.
    add_action('switch_blog', 'wp_switch_roles_and_user', 1, 2);
}
add_filter('wp_die_handler', static fn () => 'speedyspec_compat_wp_die');
add_filter('theme_page_templates', 'speedyspec_compat_posts_write_page_templates');

// init: the core types and statuses, and the two taxonomies the seeded terms are in (without rewrite rules).
create_initial_post_types();
register_taxonomy('category', 'post', ['hierarchical' => true, 'rewrite' => false, 'query_var' => false]);
register_taxonomy('post_tag', 'post', ['rewrite' => false, 'query_var' => false]);
do_action('init');

/**
 * The cache groups whose keys each step reports.
 *
 * @return list<string>
 */
function speedyspec_compat_posts_write_groups(): array
{
    return ['posts', 'post_meta', 'counts'];
}

/**
 * Prepares each scenario once the driver is loaded: a flushed cache, and the state each step reports.
 */
function speedyspec_compat_area_setup(): void
{
    global $wpdb;
    wp_cache_flush();
    SpeedySpecCompat::$report_globals = false;
    SpeedySpecCompat::$exact_bytes = true;
    // A fixture's arguments are often posts (objects): report their properties.
    SpeedySpecCompat::$arguments = static fn (array $args): array => array_map('speedyspec_compat_plain', $args);
    $last = $wpdb->num_queries;
    $kept = [];
    SpeedySpecCompat::$state = static function () use (&$last, &$kept): array
    {
        global $wpdb, $wp_object_cache;
        $state = ['queries' => $wpdb->num_queries - $last];
        foreach (speedyspec_compat_posts_write_groups() as $group)
        {
            if (!empty($wp_object_cache->cache[$group]))
            {
                $keys = array_map('strval', array_keys($wp_object_cache->cache[$group]));
                sort($keys, SORT_STRING);
                $state["cache:$group"] = $keys;
            }
        }
        // Each value kept with `as` that is new or changed, as plain data.
        foreach (SpeedySpecCompat::$vars as $name => $value)
        {
            $plain = speedyspec_compat_plain($value);
            $seen = serialize($plain);
            if (($kept[$name] ?? null) !== $seen)
            {
                $kept[$name] = $seen;
                $state["kept:$name"] = $plain;
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
