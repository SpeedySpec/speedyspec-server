<?php
require __DIR__ . '/../wordpress-stubs.php';
global $wpdb;
// wp-settings.php's wp_set_wpdb_vars() does this with wp-config.php's $table_prefix.
$wpdb->set_prefix(getenv('TABLE_PREFIX'));
wp_cache_init();
$wpdb->query("CREATE TABLE IF NOT EXISTS {$wpdb->options} (option_id bigint(20) unsigned NOT NULL auto_increment, option_name varchar(191) NOT NULL default '', option_value longtext NOT NULL, autoload varchar(20) NOT NULL default 'yes', PRIMARY KEY (option_id), UNIQUE KEY option_name (option_name), KEY autoload (autoload)) DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_520_ci");
update_option('speedyspec_worker_probe', 'from-worker');
echo (new ReflectionFunction('get_option'))->getFileName(), '|', get_option('speedyspec_worker_probe');
