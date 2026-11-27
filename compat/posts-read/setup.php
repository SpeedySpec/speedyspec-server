<?php

declare(strict_types=1);

/**
 * Sets up the post-reading scenarios the same way on both sides: WordPress's own files, the same tables and seeds,
 *  and the state each step reports.
 *
 *  `SPEEDYSPEC_COMPAT_WP_ROOT` is the release to load from and `SPEEDYSPEC_COMPAT_PORT` whether the native port is
 *  loaded. The recorder loads WordPress's hooks, object cache, `wpdb`, `meta.php`, `post.php`, `class-wp-post.php`
 *  and `class-wp-post-type.php`; the port's side loads none of the first three (they are native) and its release
 *  copies of the others have the ported functions removed. Both need MySQL, through `SPEEDYSPEC_TEST_MYSQL_URL`, on
 *  a database of the area's own (`<database>_posts_read`): the seed empties every table it creates.
 *
 *  The seed (blog 1), by ID: type, `post_name`, status, parent.
 *
 *  - 1 post `hello-world` publish; 2 page `about` publish; 3 page `team` (2); 4 page `lead` (3);
 *  - 5 page `caf%c3%a9` (an encoded name); 6 page `dup` draft and 7 page `dup` publish; 8 page `Mixed` (upper case);
 *  - 9 attachment `photo` inherit (1, image/jpeg); 10 attachment `unattached` inherit (0, image/png);
 *    11 attachment `doc` inherit (12, application/pdf); 13 attachment `trashed-file` trash (image/jpeg);
 *    25 attachment `draft-child` inherit (16, image/gif); 26 attachment `lost` inherit (999, image/webp);
 *    27 attachment `published-file` publish (0, video/mp4);
 *  - 12 post `trashed` trash, with `_wp_trash_meta_status` `draft`; 14 post private by user 2; 15 post private by
 *    user 1; 16 post `draft-post` draft by user 2 with zero dates; 24 post `future-post` future;
 *  - 17 page `loop-a` (18) and 18 page `loop-b` (17); 19 page `orphan` (999, missing); 22 page `self-parent` (22);
 *  - 20 `book` `about` and 21 `book` `chapter` (20), a type the scenarios register themselves;
 *  - 23 revision of 1.
 *
 *  Users: 1 `compat` (administrator) and 2 `writer` (author), with the roles in `wp_user_roles`. Post 1 is in
 *  category 1 and tag 2; post 1 has meta `k`, page 4 `_wp_page_template`.
 *
 *  `SPEEDYSPEC_COMPAT_MULTISITE` (false when undefined) makes the install a multisite one, as `wp-config.php` and
 *  `ms-settings.php` would: `MULTISITE`, network 1 with main site 1 as `$current_site`, and blog 1. Blog 2 has its own
 *  `wp_2_posts` (1 page `blog-two-home`, 2 post `second`, 3 page `blog-two-child` (1)), `wp_2_postmeta` and empty
 *  term tables.
 *
 *  Each step reports the queries it ran, the post cache groups, and every value kept with `as` that changed (objects
 *  as their class and properties, since the driver reports an object by its class alone). `microtime()` strings
 *  (`last_changed`) are reported as `{microtime:N}`, numbered in the order they first appear in the scenario.
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
    throw new RuntimeException('the post-reading compatibility scenarios need MySQL: set SPEEDYSPEC_TEST_MYSQL_URL');
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

/** @var array<string, string> `microtime()` strings by the placeholder each is reported as, per scenario. */
$GLOBALS['speedyspec_compat_microtimes'] = [];

/**
 * A value as plain data the driver can report: an object becomes its class and properties (a fixture callback stays
 *  its label), and a `microtime()` string its numbered placeholder.
 *
 * @return mixed
 */
function speedyspec_compat_plain($value, int $depth = 0)
{
    if (is_string($value))
    {
        if (preg_match('/^0\.\d{8} \d{10}$/', $value))
        {
            $seen = &$GLOBALS['speedyspec_compat_microtimes'];
            if (!isset($seen[$value]))
            {
                $seen[$value] = '{microtime:' . (count($seen) + 1) . '}';
            }
            return $seen[$value];
        }
        return $value;
    }
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
 * A `stdClass` with the given fields, which scenario JSON cannot carry (it decodes objects as arrays).
 */
function speedyspec_compat_object(array $fields): stdClass
{
    return (object) $fields;
}

/**
 * Caches a post as another PHP process would have: a `stdClass` with `filter` `raw`, under the post's ID in the
 *  `posts` group, with the given fields over the seeded row's (a `filter` of null leaves the property out).
 *
 * @return bool
 *   What {@link wp_cache_set()} answers.
 */
function speedyspec_compat_cache_post(int $id, array $fields)
{
    global $wpdb;
    $row = $wpdb->get_row($wpdb->prepare("SELECT * FROM $wpdb->posts WHERE ID = %d", $id), ARRAY_A) ?: ['ID' => $id];
    $row['ID'] = (int) $row['ID'];
    foreach (['post_parent', 'menu_order'] as $int)
    {
        if (isset($row[$int]))
        {
            $row[$int] = (int) $row[$int];
        }
    }
    $entry = array_merge($row, ['filter' => 'raw'], $fields);
    if (array_key_exists('filter', $fields) && $fields['filter'] === null)
    {
        unset($entry['filter']);
    }
    return wp_cache_set($id, (object) $entry, 'posts');
}

/**
 * Caches a value that is not a post (an array or a scalar) under a post's ID, as a stray or older writer might.
 *
 * @return bool
 */
function speedyspec_compat_cache_value(int $id, $value)
{
    return wp_cache_set($id, $value, 'posts');
}

/**
 * Answers `pre_wp_count_posts` with counts of its own, so wp_count_posts() skips its query.
 */
function speedyspec_compat_count_override($counts): stdClass
{
    return (object) ['publish' => 42, 'made_up' => 7];
}

/**
 * Answers `pre_wp_count_attachments` with counts of its own, so wp_count_attachments() skips its queries.
 */
function speedyspec_compat_attachments_override($counts): stdClass
{
    return (object) ['image/jpeg' => 5];
}

/**
 * Answers `pre_get_lastpostmodified` with a date of its own, so get_lastpostmodified() reads nothing.
 */
function speedyspec_compat_lastmodified_override($date): string
{
    return '2000-01-01 00:00:00';
}

/**
 * Logs a user in for the request, as `wp_set_current_user()` does after authentication.
 *
 * @return int
 *   The current user's ID.
 */
function speedyspec_compat_login(int $id): int
{
    wp_set_current_user($id);
    return get_current_user_id();
}

if (!SPEEDYSPEC_COMPAT_PORT)
{
    require ABSPATH . WPINC . '/plugin.php';
}
require ABSPATH . WPINC . '/load.php';
// The port's release copies stub these two out (speedyspec-wp-utf8-php), as wp-settings.php orders them.
require ABSPATH . WPINC . '/compat-utf8.php';
require ABSPATH . WPINC . '/compat.php';
require ABSPATH . WPINC . '/utf8.php';
require ABSPATH . WPINC . '/class-wp-error.php';
require ABSPATH . WPINC . '/formatting.php';
require ABSPATH . WPINC . '/functions.php';
require ABSPATH . WPINC . '/kses.php';
require ABSPATH . WPINC . '/class-wp-list-util.php';
require ABSPATH . WPINC . '/class-wp-meta-query.php';
require ABSPATH . WPINC . '/meta.php';
require ABSPATH . WPINC . '/class-wp-post.php';
require ABSPATH . WPINC . '/class-wp-post-type.php';
require ABSPATH . WPINC . '/post.php';
require ABSPATH . WPINC . '/revision.php';
// The term cache _prime_post_caches() and clean_post_cache() touch, and WP_Post::to_array()'s categories and tags.
require ABSPATH . WPINC . '/class-wp-term.php';
require ABSPATH . WPINC . '/class-wp-taxonomy.php';
require ABSPATH . WPINC . '/class-wp-term-query.php';
require ABSPATH . WPINC . '/taxonomy.php';
require ABSPATH . WPINC . '/category.php';
require ABSPATH . WPINC . '/category-template.php';
// The current user wp_count_posts( 'readable' ) asks about, and user_can_richedit() for the edit context.
require ABSPATH . WPINC . '/class-wp-role.php';
require ABSPATH . WPINC . '/class-wp-roles.php';
require ABSPATH . WPINC . '/class-wp-user.php';
require ABSPATH . WPINC . '/capabilities.php';
require ABSPATH . WPINC . '/user.php';
require ABSPATH . WPINC . '/pluggable.php';
require ABSPATH . WPINC . '/general-template.php';
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
// wp_cache_get_salted() and friends, which get_page_by_path() uses, as wp-settings.php loads them.
require ABSPATH . WPINC . '/cache-compat.php';
$GLOBALS['wpdb']->set_prefix('wp_');
if (SPEEDYSPEC_COMPAT_MULTISITE)
{
    // ms-settings.php: on multisite, set_prefix() leaves the blog tables to set_blog_id().
    $GLOBALS['wpdb']->set_blog_id(1, 1);
}
wp_cache_init();
// wp_start_object_cache()'s global groups.
wp_cache_add_global_groups(
    ['blog-details', 'blog-id-cache', 'blog-lookup', 'blog_meta', 'global-posts', 'image_editor', 'networks',
        'network-queries', 'sites', 'site-details', 'site-options', 'site-queries', 'site-transient', 'theme_files',
        'translation_files', 'rss', 'users', 'user-queries', 'user_meta', 'useremail', 'userlogins', 'userslugs']
);

/**
 * The tables the seed creates and empties, by name: their `CREATE TABLE` statements.
 *
 * @return array<string, string>
 */
function speedyspec_compat_posts_read_tables(): array
{
    $charset = 'DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_520_ci';
    $meta = static fn (string $table, string $id, string $object): string =>
        "CREATE TABLE IF NOT EXISTS $table ($id bigint(20) unsigned NOT NULL auto_increment, "
        . "$object bigint(20) unsigned NOT NULL default '0', meta_key varchar(255) default NULL, meta_value longtext, "
        . "PRIMARY KEY ($id), KEY $object ($object), KEY meta_key (meta_key(191))) $charset";
    // wp-admin/includes/schema.php's posts table.
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
    $tables = [
        'wp_posts' => $posts('wp_posts'),
        'wp_postmeta' => $meta('wp_postmeta', 'meta_id', 'post_id'),
        'wp_usermeta' => $meta('wp_usermeta', 'umeta_id', 'user_id'),
        'wp_users' => "CREATE TABLE IF NOT EXISTS wp_users (ID bigint(20) unsigned NOT NULL auto_increment, "
            . "user_login varchar(60) NOT NULL default '', user_pass varchar(255) NOT NULL default '', "
            . "user_nicename varchar(50) NOT NULL default '', user_email varchar(100) NOT NULL default '', "
            . "user_url varchar(100) NOT NULL default '', "
            . "user_registered datetime NOT NULL default '0000-00-00 00:00:00', "
            . "user_activation_key varchar(255) NOT NULL default '', user_status int(11) NOT NULL default '0', "
            . "display_name varchar(250) NOT NULL default '', PRIMARY KEY (ID)) $charset",
        'wp_options' => "CREATE TABLE IF NOT EXISTS wp_options (option_id bigint(20) unsigned NOT NULL auto_increment, "
            . "option_name varchar(191) NOT NULL default '', option_value longtext NOT NULL, "
            . "autoload varchar(20) NOT NULL default 'yes', PRIMARY KEY (option_id), "
            . "UNIQUE KEY option_name (option_name), KEY autoload (autoload)) $charset",
        'wp_terms' => "CREATE TABLE IF NOT EXISTS wp_terms (term_id bigint(20) unsigned NOT NULL auto_increment, "
            . "name varchar(200) NOT NULL default '', slug varchar(200) NOT NULL default '', "
            . "term_group bigint(10) NOT NULL default 0, PRIMARY KEY (term_id)) $charset",
        'wp_term_taxonomy' => "CREATE TABLE IF NOT EXISTS wp_term_taxonomy "
            . "(term_taxonomy_id bigint(20) unsigned NOT NULL auto_increment, "
            . "term_id bigint(20) unsigned NOT NULL default 0, taxonomy varchar(32) NOT NULL default '', "
            . "description longtext NOT NULL, parent bigint(20) unsigned NOT NULL default 0, "
            . "count bigint(20) NOT NULL default 0, PRIMARY KEY (term_taxonomy_id), "
            . "UNIQUE KEY term_id_taxonomy (term_id,taxonomy)) $charset",
        'wp_term_relationships' => "CREATE TABLE IF NOT EXISTS wp_term_relationships "
            . "(object_id bigint(20) unsigned NOT NULL default 0, "
            . "term_taxonomy_id bigint(20) unsigned NOT NULL default 0, term_order int(11) NOT NULL default 0, "
            . "PRIMARY KEY (object_id,term_taxonomy_id)) $charset",
        'wp_termmeta' => $meta('wp_termmeta', 'meta_id', 'term_id'),
    ];
    if (SPEEDYSPEC_COMPAT_MULTISITE)
    {
        $tables['wp_2_posts'] = $posts('wp_2_posts');
        $tables['wp_2_postmeta'] = $meta('wp_2_postmeta', 'meta_id', 'post_id');
        // Blog 2's term tables, which _prime_post_caches() reads there, empty.
        foreach (['wp_terms', 'wp_term_taxonomy', 'wp_term_relationships', 'wp_termmeta'] as $table)
        {
            $blog = str_replace('wp_', 'wp_2_', $table);
            $tables[$blog] = str_replace(" $table ", " $blog ", $tables[$table]);
        }
        $tables['wp_blogs'] = "CREATE TABLE IF NOT EXISTS wp_blogs "
            . "(blog_id bigint(20) unsigned NOT NULL auto_increment, "
            . "site_id bigint(20) unsigned NOT NULL default '0', domain varchar(200) NOT NULL default '', "
            . "path varchar(100) NOT NULL default '', "
            . "registered datetime NOT NULL default '0000-00-00 00:00:00', "
            . "last_updated datetime NOT NULL default '0000-00-00 00:00:00', public tinyint(2) NOT NULL default '1', "
            . "archived tinyint(2) NOT NULL default '0', mature tinyint(2) NOT NULL default '0', "
            . "spam tinyint(2) NOT NULL default '0', deleted tinyint(2) NOT NULL default '0', "
            . "lang_id int(11) NOT NULL default '0', PRIMARY KEY (blog_id)) $charset";
    }
    return $tables;
}

/**
 * A seeded post row: the type, name, status and parent, with the rest defaulted from the ID.
 *
 * @return array<string, int|string>
 */
function speedyspec_compat_post_row(
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
 * Creates the tables if needed, empties them, and seeds the posts, users, terms and roles the scenarios read.
 */
function speedyspec_compat_posts_read_schema(): void
{
    global $wpdb;
    $wpdb->suppress_errors(true);
    foreach (speedyspec_compat_posts_read_tables() as $table => $create)
    {
        $wpdb->query($create);
        $wpdb->query("TRUNCATE TABLE $table");
    }
    $zero = '0000-00-00 00:00:00';
    $rows = [
        speedyspec_compat_post_row(1, 'post', 'hello-world', 'publish', 0, [
            'post_title' => 'Hello "World" & <b>friends</b>',
            'post_content' => "<p>Body & <script>alert('x')</script></p>\nline two",
            'post_excerpt' => "It's <em>here</em>",
            'post_password' => '',
            'comment_count' => 3,
        ]),
        speedyspec_compat_post_row(2, 'page', 'about', 'publish', 0, ['menu_order' => 2]),
        speedyspec_compat_post_row(3, 'page', 'team', 'publish', 2),
        speedyspec_compat_post_row(4, 'page', 'lead', 'publish', 3),
        speedyspec_compat_post_row(5, 'page', 'caf%c3%a9', 'publish', 0),
        speedyspec_compat_post_row(6, 'page', 'dup', 'draft', 0),
        speedyspec_compat_post_row(7, 'page', 'dup', 'publish', 0),
        speedyspec_compat_post_row(8, 'page', 'Mixed', 'publish', 0),
        speedyspec_compat_post_row(9, 'attachment', 'photo', 'inherit', 1, ['post_mime_type' => 'image/jpeg']),
        speedyspec_compat_post_row(10, 'attachment', 'unattached', 'inherit', 0, ['post_mime_type' => 'image/png']),
        speedyspec_compat_post_row(11, 'attachment', 'doc', 'inherit', 12, ['post_mime_type' => 'application/pdf']),
        speedyspec_compat_post_row(12, 'post', 'trashed__trashed', 'trash', 0),
        speedyspec_compat_post_row(13, 'attachment', 'trashed-file', 'trash', 0, ['post_mime_type' => 'image/jpeg']),
        speedyspec_compat_post_row(14, 'post', 'private-by-author', 'private', 0, ['post_author' => 2]),
        speedyspec_compat_post_row(15, 'post', 'private-by-admin', 'private', 0),
        speedyspec_compat_post_row(16, 'post', '', 'draft', 0, [
            'post_author' => 2,
            'post_date' => $zero,
            'post_date_gmt' => $zero,
            'post_modified' => $zero,
            'post_modified_gmt' => $zero,
        ]),
        speedyspec_compat_post_row(17, 'page', 'loop-a', 'publish', 18),
        speedyspec_compat_post_row(18, 'page', 'loop-b', 'publish', 17),
        speedyspec_compat_post_row(19, 'page', 'orphan', 'publish', 999),
        speedyspec_compat_post_row(20, 'book', 'about', 'publish', 0),
        speedyspec_compat_post_row(21, 'book', 'chapter', 'publish', 20),
        speedyspec_compat_post_row(22, 'page', 'self-parent', 'publish', 22),
        speedyspec_compat_post_row(23, 'revision', '1-revision-v1', 'inherit', 1),
        speedyspec_compat_post_row(24, 'post', 'future-post', 'future', 0, [
            'post_date' => '2027-06-01 10:00:00',
            'post_date_gmt' => '2027-06-01 08:00:00',
        ]),
        speedyspec_compat_post_row(25, 'attachment', 'draft-child', 'inherit', 16, ['post_mime_type' => 'image/gif']),
        speedyspec_compat_post_row(26, 'attachment', 'lost', 'inherit', 999, ['post_mime_type' => 'image/webp']),
        speedyspec_compat_post_row(27, 'attachment', 'published-file', 'publish', 0, ['post_mime_type' => 'video/mp4']),
    ];
    foreach ($rows as $row)
    {
        $wpdb->insert('wp_posts', $row);
    }
    $meta = [[1, 'k', 'v'], [4, '_wp_page_template', 'tpl.php'], [12, '_wp_trash_meta_status', 'draft']];
    foreach ($meta as [$id, $key, $value])
    {
        $wpdb->insert('wp_postmeta', ['post_id' => $id, 'meta_key' => $key, 'meta_value' => $value]);
    }
    foreach ([1 => ['compat', 'administrator'], 2 => ['writer', 'author']] as $id => [$login, $role])
    {
        $wpdb->insert('wp_users', [
            'ID' => $id, 'user_login' => $login, 'user_nicename' => $login, 'user_email' => "$login@example.org",
            'user_registered' => '2026-01-01 00:00:00', 'display_name' => ucfirst($login),
        ]);
        $wpdb->insert('wp_usermeta', [
            'user_id' => $id, 'meta_key' => 'wp_capabilities', 'meta_value' => serialize([$role => true]),
        ]);
    }
    $wpdb->insert('wp_options', [
        'option_name' => 'wp_user_roles',
        'autoload' => 'on',
        'option_value' => serialize([
            'administrator' => ['name' => 'Administrator', 'capabilities' => [
                'read' => true, 'edit_posts' => true, 'edit_others_posts' => true, 'publish_posts' => true,
                'read_private_posts' => true, 'edit_pages' => true, 'read_private_pages' => true,
                'upload_files' => true, 'manage_options' => true,
            ]],
            'author' => ['name' => 'Author', 'capabilities' => [
                'read' => true, 'edit_posts' => true, 'edit_published_posts' => true, 'publish_posts' => true,
                'upload_files' => true,
            ]],
        ]),
    ]);
    $wpdb->insert('wp_terms', ['term_id' => 1, 'name' => 'Uncategorized', 'slug' => 'uncategorized']);
    $wpdb->insert('wp_terms', ['term_id' => 2, 'name' => 'Tagged', 'slug' => 'tagged']);
    foreach ([1 => 'category', 2 => 'post_tag'] as $id => $taxonomy)
    {
        $wpdb->insert(
            'wp_term_taxonomy',
            ['term_taxonomy_id' => $id, 'term_id' => $id, 'taxonomy' => $taxonomy, 'description' => '', 'count' => 1]
        );
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
            speedyspec_compat_post_row(1, 'page', 'blog-two-home', 'publish', 0, ['post_title' => 'Blog two']),
            speedyspec_compat_post_row(2, 'post', 'second', 'publish', 0, [
                'post_date' => '2026-03-01 10:00:00', 'post_date_gmt' => '2026-03-01 08:00:00',
            ]),
            speedyspec_compat_post_row(3, 'page', 'blog-two-child', 'publish', 1),
        ] as $row)
        {
            $wpdb->insert('wp_2_posts', $row);
        }
        $wpdb->insert('wp_2_postmeta', ['post_id' => 1, 'meta_key' => 'k', 'meta_value' => 'blog-2']);
    }
    $wpdb->suppress_errors(false);
}

// The seed runs before WordPress registers anything, as an installed site's tables exist before the request.
speedyspec_compat_posts_read_schema();

// `init` has run, so `wp_load_translations_early()` (called by wpdb on invalid text) loads no l10n.php beside the
// stubs above.
do_action('init');
add_filter('wp_die_handler', static fn () => 'speedyspec_compat_wp_die');
// default-filters.php's one filter get_page_by_path() depends on: sanitize_title_for_query() lower-cases through it.
add_filter('sanitize_title', 'sanitize_title_with_dashes', 10, 3);
// default-filters.php's init callbacks the readers depend on: the core types and statuses, and the two taxonomies
// the seeded terms are in (without rewrite rules).
create_initial_post_types();
register_taxonomy('category', 'post', ['hierarchical' => true, 'rewrite' => false, 'query_var' => false]);
register_taxonomy('post_tag', 'post', ['rewrite' => false, 'query_var' => false]);

/**
 * The cache groups each step reports.
 *
 * @return list<string>
 */
function speedyspec_compat_posts_read_groups(): array
{
    return ['posts', 'post-queries', 'counts', 'timeinfo', 'post_meta', 'category_relationships',
        'post_tag_relationships', 'general'];
}

/**
 * Prepares each scenario once the driver is loaded: a flushed cache, a logged-out reader, and the state each step
 *  reports.
 */
function speedyspec_compat_area_setup(): void
{
    global $wpdb;
    wp_cache_flush();
    $GLOBALS['speedyspec_compat_microtimes'] = [];
    SpeedySpecCompat::$report_globals = false;
    SpeedySpecCompat::$exact_bytes = true;
    // A fixture's arguments are often posts and counts (objects): report their properties.
    SpeedySpecCompat::$arguments = static fn (array $args): array => array_map('speedyspec_compat_plain', $args);
    $last = $wpdb->num_queries;
    $kept = [];
    SpeedySpecCompat::$state = static function () use (&$last, &$kept): array
    {
        global $wpdb, $wp_object_cache;
        $state = ['queries' => $wpdb->num_queries - $last];
        foreach (speedyspec_compat_posts_read_groups() as $group)
        {
            if (!empty($wp_object_cache->cache[$group]))
            {
                $state["cache:$group"] = speedyspec_compat_plain($wp_object_cache->cache[$group]);
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
