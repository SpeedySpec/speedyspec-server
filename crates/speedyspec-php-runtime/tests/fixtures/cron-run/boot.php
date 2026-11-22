<?php
// What every script here shares: WordPress's stubs, the table prefix wp-settings.php would set, the object cache,
// and the action that records each run of a due event.
require __DIR__ . '/../wordpress-stubs.php';
global $wpdb;
$wpdb->set_prefix(getenv('TABLE_PREFIX'));
wp_cache_init();
add_action('speedyspec_count', function ($tag) {
    global $wpdb;
    $wpdb->insert("{$wpdb->prefix}cron_runs", array('tag' => (string) $tag));
});
