<?php

declare(strict_types=1);

/**
 * Loads what WordPress's media metadata tests need around the native media functions: the stub test case and
 *  WordPress's own functions (kses among them) from the pinned release (`SPEEDYSPEC_WP_ROOT`).
 *
 *  The copied tests (`wordpress/media/`) call wp_read_audio_metadata(), wp_read_video_metadata(),
 *  wp_get_media_creation_timestamp() and wp_add_id3_tag_data(), which the embedded PHP answers natively over FFmpeg.
 */

require __DIR__ . '/WP_UnitTestCase.php';

foreach (['wp_read_audio_metadata', 'wp_read_video_metadata', 'wp_add_id3_tag_data', 'wp_get_media_creation_timestamp'] as $speedyspec_function)
{
    if (!function_exists($speedyspec_function) || 'speedyspec://wp-media' !== (new ReflectionFunction($speedyspec_function))->getFileName())
    {
        throw new RuntimeException("The native {$speedyspec_function}() is not loaded: run these tests in the embedded PHP.");
    }
}

$speedyspec_root = getenv('SPEEDYSPEC_WP_ROOT');
if (!$speedyspec_root)
{
    throw new RuntimeException('Set SPEEDYSPEC_WP_ROOT to the WordPress release the tests load kses from.');
}
define('ABSPATH', rtrim($speedyspec_root, '/') . '/');
define('WPINC', 'wp-includes');
define('WP_DEBUG', true);
define('DIR_TESTDATA', __DIR__ . '/wordpress/media/data');

// No database: options answer as a default install would. kses is the only caller here.
add_filter('pre_option', function ($pre, $option, $default)
{
    return ['blog_charset' => 'UTF-8'][$option] ?? ($default === false ? '' : $default);
}, 10, 3);
spl_autoload_register(function ($class)
{
    $file = ABSPATH . WPINC . '/html-api/class-' . strtolower(str_replace('_', '-', $class)) . '.php';
    if (is_file($file))
    {
        require $file;
    }
});
// plugin.php is the native Hook API's; the rest is WordPress's own.
foreach (['load.php', 'formatting.php', 'kses.php', 'functions.php', 'class-wp-token-map.php'] as $speedyspec_file)
{
    require_once ABSPATH . WPINC . '/' . $speedyspec_file;
}
