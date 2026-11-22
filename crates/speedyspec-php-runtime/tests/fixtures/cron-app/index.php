<?php
require __DIR__ . '/../wordpress-stubs.php';
global $wpdb;
$wpdb->set_prefix(getenv('TABLE_PREFIX'));
wp_cache_init();
$wpdb->query("CREATE TABLE IF NOT EXISTS {$wpdb->options} (option_id bigint(20) unsigned NOT NULL auto_increment, option_name varchar(191) NOT NULL default '', option_value longtext NOT NULL, autoload varchar(20) NOT NULL default 'yes', PRIMARY KEY (option_id), UNIQUE KEY option_name (option_name), KEY autoload (autoload)) DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_520_ci");
wp_clear_scheduled_hook('speedyspec_probe');
wp_schedule_single_event(time() + 60, 'speedyspec_probe');
echo (new ReflectionFunction('wp_next_scheduled'))->getFileName(), '|', wp_next_scheduled('speedyspec_probe') > 0 ? 'scheduled' : 'missing';
