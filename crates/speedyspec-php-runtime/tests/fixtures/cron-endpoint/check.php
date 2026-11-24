<?php
// The sites whose event ran, in run order.
require __DIR__ . '/wp-load.php';
global $wpdb;
echo json_encode($wpdb->get_col("SELECT tag FROM {$wpdb->base_prefix}cron_runs ORDER BY id"));
