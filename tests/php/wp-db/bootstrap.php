<?php

declare(strict_types=1);

/**
 * Loads what WordPress's database tests need around the native `wpdb`: the hook tests' bootstrap, WordPress's
 * test helpers, the few WordPress functions `wpdb` calls, and a fresh `wptests_` schema.
 */

require dirname(__DIR__) . '/bootstrap.php';

if (!class_exists('wpdb'))
{
    throw new RuntimeException('The native wpdb is not loaded: run these tests in the embedded PHP.');
}

$speedyspec_url = getenv('SPEEDYSPEC_TEST_MYSQL_URL');
if (!$speedyspec_url)
{
    throw new RuntimeException('The database tests need MySQL: set SPEEDYSPEC_TEST_MYSQL_URL.');
}
$speedyspec_db = parse_url($speedyspec_url);
foreach ([
    'ABSPATH' => sys_get_temp_dir() . '/speedyspec-wp-db-tests/',
    'WPINC' => 'wp-includes',
    'WP_DEBUG' => false,
    'WP_DEBUG_DISPLAY' => false,
    'DB_NAME' => ltrim($speedyspec_db['path'] ?? '', '/'),
    'DB_USER' => urldecode($speedyspec_db['user'] ?? ''),
    'DB_PASSWORD' => urldecode($speedyspec_db['pass'] ?? ''),
    'DB_HOST' => ($speedyspec_db['host'] ?? 'localhost') . (isset($speedyspec_db['port']) ? ':' . $speedyspec_db['port'] : ''),
    'DB_CHARSET' => 'utf8mb4',
    'DB_COLLATE' => '',
    'WP_TESTS_TABLE_PREFIX' => 'wptests_',
    'WP_CONTENT_DIR' => sys_get_temp_dir() . '/speedyspec-wp-db-tests/wp-content',
] as $speedyspec_name => $speedyspec_value)
{
    if (!defined($speedyspec_name))
    {
        define($speedyspec_name, $speedyspec_value);
    }
}

/**
 * Fires `doing_it_wrong_run`, as WordPress's `_doing_it_wrong()` does, so the test case sees each notice.
 */
function _doing_it_wrong($function_name, $message, $version)
{
    do_action('doing_it_wrong_run', $function_name, $message, $version);
}

/**
 * Fires `deprecated_function_run`, as WordPress's `_deprecated_function()` does.
 */
function _deprecated_function($function_name, $version, $replacement = '')
{
    do_action('deprecated_function_run', $function_name, $replacement, $version);
}

function __($text, $domain = 'default')
{
    return $text;
}

function wp_load_translations_early()
{
}

/**
 * Ends a test where WordPress would end the page, as WordPress's test suite does.
 */
class WPDieException extends Exception
{
}

function wp_die($message = '')
{
    throw new WPDieException(is_string($message) ? $message : '');
}

function dead_db()
{
    throw new RuntimeException('dead_db');
}

function absint($maybeint)
{
    return abs((int) $maybeint);
}

function is_multisite()
{
    return false;
}

function is_main_site($site_id = null, $network_id = null)
{
    return true;
}

function is_main_network($network_id = null)
{
    return true;
}

function wp_is_large_network($using = 'sites', $network_id = null)
{
    return false;
}

function mbstring_binary_safe_encoding($reset = false)
{
}

function reset_mbstring_encoding()
{
}

/**
 * Stands in for WordPress's error object, which `wpdb` returns for some failures.
 */
class WP_Error
{
    public $errors = array();

    public function __construct(public $code = '', public $message = '', public $data = '')
    {
        if ('' !== $code)
        {
            $this->errors[$code][] = $message;
        }
    }

    public function get_error_message()
    {
        return $this->message;
    }

    public function get_error_code()
    {
        return $this->code;
    }
}

function is_wp_error($thing)
{
    return $thing instanceof WP_Error;
}

// The dbDelta tests load wp-admin/includes/upgrade.php themselves; dbDelta() is already here.
$speedyspec_upgrade = ABSPATH . 'wp-admin/includes/upgrade.php';
if (!is_file($speedyspec_upgrade))
{
    mkdir(dirname($speedyspec_upgrade), 0777, true);
    file_put_contents($speedyspec_upgrade, "<?php\n");
}

// Copied unchanged from WordPress 7.1.5's wp-includes/formatting.php.
/**
 * Escapes data for use in a MySQL query.
 *
 * Usually you should prepare queries using wpdb::prepare().
 * Sometimes, spot-escaping is required or useful. One example
 * is preparing an array for use in an IN clause.
 *
 * NOTE: Since 4.8.3, '%' characters will be replaced with a placeholder string,
 * this prevents certain SQLi attacks from taking place. This change in behavior
 * may cause issues for code that expects the return value of esc_sql() to be usable
 * for other purposes.
 *
 * @since 2.8.0
 *
 * @global wpdb $wpdb WordPress database abstraction object.
 *
 * @param string|array $data Unescaped data.
 * @return string|array Escaped data, in the same type as supplied.
 */
function esc_sql( $data ) {
	global $wpdb;
	return $wpdb->_escape( $data );
}

require dirname(__DIR__) . '/includes/wpdb-exposed.php';
require dirname(__DIR__) . '/includes/dbdelta.php';
require dirname(__DIR__) . '/includes/wp-db-schema.php';
require dirname(__DIR__) . '/includes/list-util.php';

$table_prefix = 'wptests_';
$GLOBALS['table_prefix'] = $table_prefix;
$GLOBALS['wpdb']->set_prefix($table_prefix);

// WordPress's wp_set_wpdb_vars() field types, as every request sets them.
$wpdb = $GLOBALS['wpdb'];
$wpdb->field_types = array(
        'post_author'      => '%d',
        'post_parent'      => '%d',
        'menu_order'       => '%d',
        'term_id'          => '%d',
        'term_group'       => '%d',
        'term_taxonomy_id' => '%d',
        'parent'           => '%d',
        'count'            => '%d',
        'object_id'        => '%d',
        'term_order'       => '%d',
        'ID'               => '%d',
        'comment_ID'       => '%d',
        'comment_post_ID'  => '%d',
        'comment_parent'   => '%d',
        'user_id'          => '%d',
        'link_id'          => '%d',
        'link_owner'       => '%d',
        'link_rating'      => '%d',
        'option_id'        => '%d',
        'blog_id'          => '%d',
        'meta_id'          => '%d',
        'post_id'          => '%d',
        'user_status'      => '%d',
        'umeta_id'         => '%d',
        'comment_karma'    => '%d',
        'comment_count'    => '%d',
        // Multisite:
        'active'           => '%d',
        'cat_id'           => '%d',
        'deleted'          => '%d',
        'lang_id'          => '%d',
        'mature'           => '%d',
        'public'           => '%d',
        'site_id'          => '%d',
        'spam'             => '%d',
    );

$GLOBALS['wpdb']->suppress_errors(true);
foreach ($GLOBALS['wpdb']->get_col("SHOW TABLES LIKE 'wptests\\_%'") as $speedyspec_table)
{
    $GLOBALS['wpdb']->query("DROP TABLE IF EXISTS `$speedyspec_table`");
}
foreach (array_filter(array_map('trim', explode(';', wp_get_db_schema('all')))) as $speedyspec_create)
{
    $GLOBALS['wpdb']->query($speedyspec_create);
}
$GLOBALS['wpdb']->insert(
    $GLOBALS['wpdb']->users,
    array('user_login' => 'admin', 'user_email' => 'admin@example.org', 'display_name' => 'admin', 'user_registered' => '2020-01-01 00:00:00')
);
$GLOBALS['wpdb']->suppress_errors(false);
// WordPress's tests run with WP_DEBUG and WP_DEBUG_DISPLAY, under which wpdb shows its errors.
$GLOBALS['wpdb']->show_errors();
