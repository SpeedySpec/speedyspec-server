<?php
// The native post writers' cron port in a worker: `_future_post_hook()`, which wp_insert_post() reaches for a future
// post through `future_{$post_type}`, schedules through the runtime's binding (the Rust cron core).
require __DIR__ . '/../wordpress-stubs.php';
global $wpdb;
$wpdb->set_prefix(getenv('TABLE_PREFIX'));
wp_cache_init();
$wpdb->query("CREATE TABLE IF NOT EXISTS {$wpdb->options} (option_id bigint(20) unsigned NOT NULL auto_increment, option_name varchar(191) NOT NULL default '', option_value longtext NOT NULL, autoload varchar(20) NOT NULL default 'yes', PRIMARY KEY (option_id), UNIQUE KEY option_name (option_name), KEY autoload (autoload)) DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_520_ci");

// A site in UTC: the post's local date is its GMT date (formatting.php is not loaded).
function get_gmt_from_date($date_string, $format = 'Y-m-d H:i:s') { return $date_string; }

_set_cron_array(array());
$at = time() + 3600;
$post = new stdClass();
$post->ID = 41;
$post->post_type = 'post';
$post->post_date = gmdate('Y-m-d H:i:s', $at);
// Twice, as an insert and then an update of the same future post: each clears, then schedules.
_future_post_hook(0, $post);
_future_post_hook(0, $post);
$events = 0;
foreach (_get_cron_array() as $timestamp => $hooks) {
    $events += count($hooks['publish_future_post'] ?? array());
}
echo json_encode(array(
    'writer' => (new ReflectionFunction('_future_post_hook'))->getFileName(),
    'events' => $events,
    'next' => wp_next_scheduled('publish_future_post', array(41)) === $at,
    // Another event for the same hook and arguments within ten minutes is a duplicate.
    'duplicate' => wp_schedule_single_event($at + 300, 'publish_future_post', array(41)),
));
