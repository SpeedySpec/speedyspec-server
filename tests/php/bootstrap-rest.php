<?php

declare(strict_types=1);

/**
 * Loads what WordPress's REST server tests need around the shim: the stub test case, WordPress's REST test case and
 * spy server, and WordPress's own files from the pinned release (`SPEEDYSPEC_WP_ROOT`).
 *
 * `WP_REST_Server` is SpeedySpec's shim (declared by the REST area), never WordPress's class-wp-rest-server.php.
 * plugin.php is the native Hook API's. The options are an in-memory table (the tests that need the database were
 * removed from the copy): WordPress's option.php reads them from the object cache, over a database stand-in that
 * answers nothing and accepts every write. The copied tests are those of
 * tests/phpunit/tests/rest-api/rest-server.php that need no database; `wordpress.rs` names the ones that fail on
 * purpose.
 */

require __DIR__ . '/WP_UnitTestCase.php';

if (!class_exists('WP_REST_Server', false) || 'speedyspec://rest/server' !== (new ReflectionClass('WP_REST_Server'))->getFileName())
{
    throw new RuntimeException('The WP_REST_Server shim is not loaded: run these tests in the embedded PHP with the REST area.');
}

$speedyspec_root = getenv('SPEEDYSPEC_WP_ROOT');
if (!$speedyspec_root)
{
    throw new RuntimeException('Set SPEEDYSPEC_WP_ROOT to the WordPress release.');
}
define('ABSPATH', rtrim($speedyspec_root, '/') . '/');
define('WPINC', 'wp-includes');
define('WP_DEBUG', true);
define('DIR_TESTROOT', __DIR__);
define('WP_CONTENT_DIR', ABSPATH . 'wp-content');
define('WP_CONTENT_URL', 'http://example.org/wp-content');

$GLOBALS['speedyspec_options'] = [
    'home'                 => 'http://example.org',
    'siteurl'              => 'http://example.org',
    'blogname'             => 'Test Blog',
    'blogdescription'      => '',
    'permalink_structure'  => '',
    'gmt_offset'           => 0,
    'timezone_string'      => '',
    'site_icon'            => 0,
    'WPLANG'               => '',
    'blog_charset'         => 'UTF-8',
];

/**
 * A database that answers nothing and accepts every write: options are read from the object cache, primed from the
 * table above, and WordPress's option.php keeps the cache current on writes.
 */
// wpdb's output constants, which class-wpdb.php (not loaded) defines.
define('OBJECT', 'OBJECT');
define('OBJECT_K', 'OBJECT_K');
define('ARRAY_A', 'ARRAY_A');
define('ARRAY_N', 'ARRAY_N');

final class SpeedySpec_NullWpdb
{
    public $options = 'wp_options';
    public $prefix = 'wp_';
    public $base_prefix = 'wp_';
    public $insert_id = 0;
    public $last_error = '';
    public $num_queries = 0;
    public $blogid = 1;
    public $siteid = 1;
    public function prepare($query, ...$args) { return vsprintf(str_replace(['%d', '%s', '%f', '%i'], ['%d', "'%s'", '%F', '`%s`'], $query), is_array($args[0] ?? null) ? $args[0] : $args); }
    public function get_row($query = null, $output = OBJECT, $y = 0) { return null; }
    public function get_var($query = null, $x = 0, $y = 0) { return null; }
    public function get_col($query = null, $x = 0) { return []; }
    public function get_results($query = null, $output = OBJECT) { return []; }
    public function query($query) { return 1; }
    public function insert($table, $data, $format = null) { return 1; }
    public function update($table, $data, $where, $format = null, $where_format = null) { return 1; }
    public function delete($table, $where, $where_format = null) { return 1; }
    public function replace($table, $data, $format = null) { return 1; }
    public function suppress_errors($suppress = true) { return false; }
    public function esc_like($text) { return addcslashes($text, '_%\\'); }
    public function get_blog_prefix($blog_id = null) { return 'wp_'; }
    public function _escape($data) { return is_array($data) ? array_map([$this, '_escape'], $data) : addslashes((string) $data); }
    public function escape($data) { return $this->_escape($data); }
}

foreach ([
    'load.php', 'compat-utf8.php', 'utf8.php', 'compat.php', 'default-constants.php', 'class-wp-error.php',
    'functions.php', 'cache.php', 'class-wp-list-util.php', 'pomo/translations.php', 'json-schema.php',
    'formatting.php', 'kses.php', 'l10n.php', 'link-template.php', 'general-template.php', 'http.php',
    'class-wp-http-response.php', 'rest-api/class-wp-rest-response.php', 'rest-api/class-wp-rest-request.php',
    'rest-api.php', 'class-wp-user.php', 'class-wp-roles.php', 'class-wp-role.php', 'capabilities.php', 'user.php',
    'pluggable.php', 'class-wp-theme.php', 'theme.php', 'post.php', 'shortcodes.php', 'media.php',
] as $speedyspec_file)
{
    require_once ABSPATH . WPINC . '/' . $speedyspec_file;
}

wp_initial_constants();
wp_cache_init();
$GLOBALS['wpdb'] = new SpeedySpec_NullWpdb();
wp_cache_set('alloptions', $GLOBALS['speedyspec_options'], 'options');
wp_cache_set('notoptions', [], 'options');
wp_plugin_directory_constants();
wp_cookie_constants();
require __DIR__ . '/includes/mock-action.php';
require __DIR__ . '/includes/testcase-rest-api.php';
require __DIR__ . '/includes/spy-rest-server.php';

// WordPress's default REST filters (default-filters.php hooks them on rest_api_init), as its test environment has.
add_action('rest_api_init', 'rest_api_default_filters', 10, 1);

// As WordPress's test bootstrap: every REST server the tests build is the spy.
add_filter('wp_rest_server_class', static fn () => 'Spy_REST_Server');

// The server variables WordPress's test environment sets (tests_reset__SERVER).
$_SERVER['HTTP_HOST'] = 'example.org';
$_SERVER['REMOTE_ADDR'] = '127.0.0.1';
$_SERVER['REQUEST_METHOD'] = 'GET';
$_SERVER['REQUEST_URI'] = '';
$_SERVER['SERVER_NAME'] = 'example.org';
$_SERVER['SERVER_PORT'] = '80';
$_SERVER['SERVER_PROTOCOL'] = 'HTTP/1.1';

// WordPress has loaded: operations may initialize.
do_action('init');
