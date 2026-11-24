<?php
// Both sites' options tables, the runs table, and one due event on each site.
require __DIR__ . '/wp-load.php';
global $wpdb;
foreach (array($wpdb->base_prefix . 'options', $wpdb->base_prefix . '2_options') as $table) {
    $wpdb->query("DROP TABLE IF EXISTS $table");
    $wpdb->query("CREATE TABLE $table (option_id bigint(20) unsigned NOT NULL auto_increment, option_name varchar(191) NOT NULL default '', option_value longtext NOT NULL, autoload varchar(20) NOT NULL default 'yes', PRIMARY KEY (option_id), UNIQUE KEY option_name (option_name), KEY autoload (autoload)) DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_520_ci");
}
$wpdb->query("DROP TABLE IF EXISTS {$wpdb->base_prefix}cron_runs");
$wpdb->query("CREATE TABLE {$wpdb->base_prefix}cron_runs (id bigint(20) unsigned NOT NULL auto_increment, tag varchar(20) NOT NULL, PRIMARY KEY (id)) DEFAULT CHARSET=utf8mb4");
foreach (array(1, 2) as $site) {
    switch_to_blog($site);
    _set_cron_array(array());
    wp_schedule_single_event(time() - 30, 'speedyspec_site_hook');
    restore_current_blog();
}
echo 'seeded';
