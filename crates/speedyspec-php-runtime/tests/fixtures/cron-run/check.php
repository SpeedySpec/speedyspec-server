<?php
require __DIR__ . '/boot.php';
global $wpdb;
$hourly = array();
foreach (_get_cron_array() as $timestamp => $hooks) {
    foreach ($hooks['speedyspec_count'] ?? array() as $event) {
        if (array('hourly') === $event['args']) {
            $hourly[] = $timestamp > time() ? 'future' : 'past';
        }
    }
}
echo json_encode(array(
    'runs' => (int) $wpdb->get_var("SELECT COUNT(*) FROM {$wpdb->prefix}cron_runs"),
    'distinct' => (int) $wpdb->get_var("SELECT COUNT(DISTINCT tag) FROM {$wpdb->prefix}cron_runs"),
    'hourly' => $hourly,
));
