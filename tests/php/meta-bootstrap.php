<?php

declare(strict_types=1);

/**
 * Loads what WordPress's meta tests need around the native meta functions: the stub test case and mocks, then the
 *  metadata compatibility setup with the port loaded (WordPress's post, comment, taxonomy, user and capability files,
 *  the meta tables, and the objects its scenarios seed).
 *
 *  The factory stores the objects the tests make as rows (`SPEEDYSPEC_FACTORY_ROWS`, see includes/factory.php).
 *  `WP_TESTS_MULTISITE=1` makes the install a multisite one, as WordPress's own bootstrap does, through the setup's
 *  multisite branch (network 1, blogs 1 and 2, `wp_blogmeta`).
 *
 *  Left out of `wordpress/meta`: `query.php` (`WP_Meta_Query` is not ported).
 */

require __DIR__ . '/WP_UnitTestCase.php';
require __DIR__ . '/includes/mock-action.php';
require __DIR__ . '/includes/mock-invokable.php';

if (!function_exists('add_metadata') || !class_exists('wpdb'))
{
    throw new RuntimeException('The native meta functions are not loaded: run these tests in the embedded PHP.');
}

$speedyspec_root = getenv('SPEEDYSPEC_META_WP_ROOT');
if (!$speedyspec_root)
{
    throw new RuntimeException('Set SPEEDYSPEC_META_WP_ROOT to the prepared WordPress release.');
}
define('SPEEDYSPEC_COMPAT_WP_ROOT', rtrim($speedyspec_root, '/') . '/');
define('SPEEDYSPEC_COMPAT_PORT', true);
define('SPEEDYSPEC_COMPAT_MULTISITE', '1' === getenv('WP_TESTS_MULTISITE'));
define('SPEEDYSPEC_FACTORY_ROWS', true);
define('DIR_TESTDATA', __DIR__ . '/data');
// `Tests_Meta_Register_Meta::wpTearDownAfterClass()` deletes its fixtures through `wp_delete_post()` and the other
// delete functions, which need cron, `WP_Query` and wp-admin (none loaded). Its rows are left to the next run's
// `speedyspec_compat_meta_schema()`, which empties the tables.
define('SPEEDYSPEC_SKIP_TEAR_DOWN_AFTER_CLASS', array('Tests_Meta_Register_Meta'));
require dirname(__DIR__, 2) . '/compat/metadata/setup.php';
// The post meta functions ask whether a post is a revision.
require_once ABSPATH . WPINC . '/revision.php';
// register_post_type(), for the revisions check in register_meta().
require_once ABSPATH . WPINC . '/class-wp-post-type.php';
speedyspec_compat_meta_schema();
if (is_multisite())
{
    // The network options `is_site_meta_supported()` keeps (`site_meta_supported`), emptied for the run.
    $wpdb = $GLOBALS['wpdb'];
    $wpdb->query(
        "CREATE TABLE IF NOT EXISTS wp_sitemeta (meta_id bigint(20) unsigned NOT NULL auto_increment, "
        . "site_id bigint(20) unsigned NOT NULL default '0', meta_key varchar(255) default NULL, meta_value longtext, "
        . "PRIMARY KEY (meta_id), KEY meta_key (meta_key(191)), KEY site_id (site_id)) "
        . "DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_520_ci"
    );
    $wpdb->query('TRUNCATE TABLE wp_sitemeta');
}
wp_cache_flush();
