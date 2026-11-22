<?php
require __DIR__ . '/boot.php';
global $wpdb;
$wpdb->query("CREATE TABLE IF NOT EXISTS {$wpdb->options} (option_id bigint(20) unsigned NOT NULL auto_increment, option_name varchar(191) NOT NULL default '', option_value longtext NOT NULL, autoload varchar(20) NOT NULL default 'yes', PRIMARY KEY (option_id), UNIQUE KEY option_name (option_name), KEY autoload (autoload)) DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_520_ci");
$wpdb->query("DROP TABLE IF EXISTS {$wpdb->prefix}cron_runs");
$wpdb->query("CREATE TABLE {$wpdb->prefix}cron_runs (id bigint(20) unsigned NOT NULL auto_increment, tag varchar(20) NOT NULL, PRIMARY KEY (id)) DEFAULT CHARSET=utf8mb4");
_set_cron_array(array());
for ($i = 0; $i < 20; $i++) {
    wp_schedule_single_event(time() - 100 - $i, 'speedyspec_count', array($i));
}
wp_schedule_event(time() - 500, 'hourly', 'speedyspec_count', array('hourly'));
echo count(_get_cron_array());
