<?php

declare(strict_types=1);

/**
 * Loads what WordPress's token map test needs around the token map: the stub test case, the few WordPress
 * functions it reaches, then the prepared release's class-wp-token-map.php and the HTML5 named character reference
 * table.
 *
 * The copied test is WordPress 7.1.5's `tests/phpunit/tests/wp-token-map/wpTokenMap.php`, from wordpress-develop at
 * tag 7.1.5, named after its class, with `data_html5_test_dataset()` made static for the pinned PHPUnit 11.
 */

require __DIR__ . '/bootstrap.php';

$speedyspec_root = getenv('SPEEDYSPEC_TOKEN_MAP_WP_ROOT');
if (!$speedyspec_root)
{
    throw new RuntimeException('Set SPEEDYSPEC_TOKEN_MAP_WP_ROOT to the prepared WordPress release.');
}

define('ABSPATH', rtrim($speedyspec_root, '/') . '/');
define('WPINC', 'wp-includes');
define('DIR_TESTDATA', __DIR__ . '/data');

function _doing_it_wrong($function_name, $message, $version): void
{
    do_action('doing_it_wrong_run', $function_name, $message, $version);
}

function __($text, $domain = 'default')
{
    return $text;
}

/**
 * WordPress's wp_json_file_decode() for a readable file (functions.php is not loaded).
 *
 * @param string $filename
 *     The JSON file.
 * @param array $options
 *     `associative`, as WordPress takes it.
 * @return mixed
 *     The decoded value, or null.
 */
function wp_json_file_decode($filename, $options = array())
{
    $filename = realpath($filename);
    if (!$filename)
    {
        return null;
    }
    $decoded = json_decode(file_get_contents($filename), !empty($options['associative']));
    return JSON_ERROR_NONE === json_last_error() ? $decoded : null;
}

require_once ABSPATH . WPINC . '/class-wp-token-map.php';
require_once ABSPATH . WPINC . '/html-api/html5-named-character-references.php';
