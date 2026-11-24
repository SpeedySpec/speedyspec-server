<?php
// Stands in for WordPress's boot on a two-site network: picks the site from HTTP_HOST and REQUEST_URI as
// ms-settings.php does (the longest matching path), with `mapped.test` booting site 1 as a domain mapping would.
// The endpoint under test (speedyspec-cron.php, copied beside this file) requires it.
$GLOBALS['speedyspec_sites'] = array(1 => array('example.test', '/'), 2 => array('example.test', '/site2/'));
$speedyspec_host = $_SERVER['HTTP_HOST'] ?? 'example.test';
$speedyspec_uri  = $_SERVER['REQUEST_URI'] ?? '/';
$GLOBALS['blog_id'] = 1;
if ($speedyspec_host !== 'mapped.test') {
    $best = -1;
    foreach ($GLOBALS['speedyspec_sites'] as $id => $site) {
        if ($site[0] === $speedyspec_host && str_starts_with($speedyspec_uri, $site[1]) && strlen($site[1]) > $best) {
            $GLOBALS['blog_id'] = $id;
            $best = strlen($site[1]);
        }
    }
}
$GLOBALS['_wp_switched_stack'] = array();

define('MULTISITE', true);
function is_multisite() { return true; }
function get_current_blog_id() { return (int) $GLOBALS['blog_id']; }
require getenv('SPEEDYSPEC_TEST_STUBS');

function get_sites($args = array()) {
    $sites = array();
    foreach ($GLOBALS['speedyspec_sites'] as $id => $row) {
        $site = new stdClass(); $site->blog_id = (string) $id; $site->domain = $row[0]; $site->path = $row[1];
        $sites[] = $site;
    }
    return $sites;
}
function switch_to_blog($new_blog_id) {
    global $wpdb;
    $GLOBALS['_wp_switched_stack'][] = $GLOBALS['blog_id'];
    $GLOBALS['blog_id'] = (int) $new_blog_id;
    $wpdb->set_blog_id((int) $new_blog_id);
    wp_cache_switch_to_blog((int) $new_blog_id);
    return true;
}
function restore_current_blog() {
    global $wpdb;
    $previous = array_pop($GLOBALS['_wp_switched_stack']);
    $GLOBALS['blog_id'] = $previous;
    $wpdb->set_blog_id($previous);
    wp_cache_switch_to_blog($previous);
    return true;
}

global $wpdb;
$wpdb->set_prefix(getenv('TABLE_PREFIX'));
$wpdb->set_blog_id($GLOBALS['blog_id']);
wp_cache_init();
wp_cache_switch_to_blog($GLOBALS['blog_id']);
// Each run records the site it ran in.
add_action('speedyspec_site_hook', function () {
    global $wpdb;
    $wpdb->insert("{$wpdb->base_prefix}cron_runs", array('tag' => (string) get_current_blog_id()));
});
