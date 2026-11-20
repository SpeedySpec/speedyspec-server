<?php

declare(strict_types=1);

/**
 * Sets up WordPress's `wpdb` scenarios: the same tables, stubs and state report on both sides.
 *
 *  The recorder loads it after WordPress's own `plugin.php` and `class-wpdb.php`; the replay loads only it, since
 *  the port's `wpdb` is native. Both need MySQL, through `SPEEDYSPEC_TEST_MYSQL_URL`.
 */

$speedyspec_url = getenv('SPEEDYSPEC_TEST_MYSQL_URL');
if (!$speedyspec_url)
{
    throw new RuntimeException('the wp-db compatibility scenarios need MySQL: set SPEEDYSPEC_TEST_MYSQL_URL');
}
$speedyspec_db = parse_url($speedyspec_url);

foreach ([
    'ABSPATH' => '/tmp/speedyspec-compat/',
    'WPINC' => 'wp-includes',
    'WP_DEBUG' => false,
    'WP_DEBUG_DISPLAY' => false,
    'DB_NAME' => ltrim($speedyspec_db['path'] ?? '', '/'),
    'DB_USER' => urldecode($speedyspec_db['user'] ?? ''),
    'DB_PASSWORD' => urldecode($speedyspec_db['pass'] ?? ''),
    'DB_HOST' => ($speedyspec_db['host'] ?? 'localhost') . (isset($speedyspec_db['port']) ? ':' . $speedyspec_db['port'] : ''),
    'DB_CHARSET' => 'utf8mb4',
    'DB_COLLATE' => '',
] as $speedyspec_name => $speedyspec_value)
{
    if (!defined($speedyspec_name))
    {
        define($speedyspec_name, $speedyspec_value);
    }
}

/**
 * Records a WordPress notice as an invocation, so a scenario shows which notices a call raised.
 */
function speedyspec_compat_notice(string $label, array $args): void
{
    if (class_exists('SpeedySpecCompat', false))
    {
        SpeedySpecCompat::$invocations[] = ['fixture' => $label, 'args' => SpeedySpecCompat::normalise($args)];
    }
}

if (!function_exists('is_multisite'))
{
    function is_multisite()
    {
        return false;
    }
}
if (!function_exists('_doing_it_wrong'))
{
    function _doing_it_wrong($function_name, $message, $version)
    {
        speedyspec_compat_notice('_doing_it_wrong', [$function_name, $message, $version]);
    }
}
if (!function_exists('_deprecated_function'))
{
    function _deprecated_function($function_name, $version, $replacement = '')
    {
        speedyspec_compat_notice('_deprecated_function', [$function_name, $version, $replacement]);
    }
}
if (!function_exists('__'))
{
    function __($text, $domain = 'default')
    {
        return $text;
    }
}
if (!function_exists('wp_load_translations_early'))
{
    function wp_load_translations_early()
    {
    }
}
if (!function_exists('wp_debug_backtrace_summary'))
{
    function wp_debug_backtrace_summary($ignore_class = null, $skip_frames = 0, $pretty = true)
    {
        return 'compat';
    }
}
if (!function_exists('wp_die'))
{
    function wp_die($message = '')
    {
        throw new RuntimeException('wp_die: ' . (is_string($message) ? $message : ''));
    }
}
if (!function_exists('dead_db'))
{
    function dead_db()
    {
        throw new RuntimeException('dead_db');
    }
}
if (!class_exists('WP_Error', false))
{
    /**
     * Stands in for WordPress's error object, which `wpdb` returns for some failures.
     */
    class WP_Error
    {
        public function __construct(public $code = '', public $message = '', public $data = '')
        {
        }

        public function get_error_message()
        {
            return $this->message;
        }
    }
}
if (!function_exists('is_wp_error'))
{
    function is_wp_error($thing)
    {
        return $thing instanceof WP_Error;
    }
}
if (!function_exists('mbstring_binary_safe_encoding'))
{
    function mbstring_binary_safe_encoding($reset = false)
    {
    }
}
if (!function_exists('reset_mbstring_encoding'))
{
    function reset_mbstring_encoding()
    {
    }
}
if (!function_exists('absint'))
{
    function absint($maybeint)
    {
        return abs((int) $maybeint);
    }
}

if (!isset($GLOBALS['wpdb']))
{
    $GLOBALS['wpdb'] = new wpdb(DB_USER, DB_PASSWORD, DB_NAME, DB_HOST);
}
$GLOBALS['wpdb']->set_prefix('wp_');
$GLOBALS['EZSQL_ERROR'] = [];

/**
 * Prepares each scenario once the driver is loaded: fresh tables, and the state each step reports.
 */
function speedyspec_compat_area_setup(): void
{
    global $wpdb;
    $wpdb->suppress_errors(true);
    $wpdb->query('DROP TABLE IF EXISTS wp_compat_items');
    $wpdb->query('DROP TABLE IF EXISTS wp_compat_latin1');
    $wpdb->query(
        'CREATE TABLE wp_compat_items (id bigint(20) unsigned NOT NULL AUTO_INCREMENT, name varchar(20) NOT NULL, '
        . 'value longtext, PRIMARY KEY (id)) DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_520_ci'
    );
    $wpdb->query('CREATE TABLE wp_compat_latin1 (name varchar(20)) DEFAULT CHARSET=latin1');
    $wpdb->suppress_errors(false);
    $wpdb->flush();
    $GLOBALS['EZSQL_ERROR'] = [];
    SpeedySpecCompat::$report_globals = false;
    SpeedySpecCompat::$placeholder = $wpdb->placeholder_escape();
    $start = $wpdb->num_queries;
    ob_start();
    SpeedySpecCompat::$state = static function () use ($start): array
    {
        global $wpdb;
        $output = ob_get_contents();
        if (false !== $output)
        {
            ob_clean();
        }
        return [
            'last_query' => $wpdb->last_query,
            'last_error' => $wpdb->last_error,
            'num_rows' => $wpdb->num_rows,
            'rows_affected' => $wpdb->rows_affected,
            'insert_id' => $wpdb->insert_id,
            'func_call' => $wpdb->func_call,
            'prefix' => $wpdb->prefix,
            'base_prefix' => $wpdb->base_prefix,
            'blogid' => $wpdb->blogid,
            'posts' => $wpdb->posts,
            'users' => $wpdb->users,
            'last_result' => array_map(static fn ($row) => get_object_vars($row), (array) $wpdb->last_result),
            'num_queries' => $wpdb->num_queries - $start,
            'output' => $output,
            'errors' => count($GLOBALS['EZSQL_ERROR'] ?? []),
        ];
    };
}
