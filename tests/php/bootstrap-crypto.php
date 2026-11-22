<?php

declare(strict_types=1);

/**
 * Loads what WordPress's sodium tests need around the native functions: the stub test case and WordPress's own
 *  functions from a release prepared without the ported ones (`SPEEDYSPEC_WP_ROOT`, see
 *  `speedyspec_wp_crypto_php::prepare_release`).
 *
 *  The copied tests are `tests/phpunit/tests/functions/wpVerifyFastHash.php` and the two signature tests of
 *  `tests/phpunit/tests/file.php`, from wordpress-develop trunk (ce78d2d6eb), unchanged.
 *  `test_wp_verify_fast_hash_verifies_phpass_hash` is expected to fail: SpeedySpec carries no phpass, so a phpass hash
 *  never verifies.
 */

require __DIR__ . '/WP_UnitTestCase.php';

foreach (['wp_fast_hash', 'wp_verify_fast_hash', 'wp_trusted_keys', 'verify_file_signature'] as $speedyspec_function)
{
    if (!function_exists($speedyspec_function) || 'speedyspec://wp-crypto' !== (new ReflectionFunction($speedyspec_function))->getFileName())
    {
        throw new RuntimeException("The native {$speedyspec_function}() is not loaded: run these tests in the embedded PHP.");
    }
}

$speedyspec_root = getenv('SPEEDYSPEC_WP_ROOT');
if (!$speedyspec_root)
{
    throw new RuntimeException('Set SPEEDYSPEC_WP_ROOT to the prepared WordPress release.');
}
define('ABSPATH', rtrim($speedyspec_root, '/') . '/');
define('WPINC', 'wp-includes');
define('WP_DEBUG', true);
// wp_tempnam() would otherwise ask the uploads option for a temporary directory; there is no database here.
define('WP_TEMP_DIR', sys_get_temp_dir());
define('WP_CONTENT_DIR', WP_TEMP_DIR);
define('WP_CONTENT_URL', 'http://example.org/wp-content');

// plugin.php is the native Hook API's; the rest is WordPress's own, without the ported functions.
foreach (['load.php', 'compat-utf8.php', 'utf8.php', 'compat.php', 'class-wp-error.php', 'class-wp-user.php', 'functions.php', 'formatting.php', 'kses.php', 'pomo/translations.php', 'l10n.php', 'user.php', 'pluggable.php'] as $speedyspec_file)
{
    require_once ABSPATH . WPINC . '/' . $speedyspec_file;
}
require_once ABSPATH . 'wp-admin/includes/file.php';

// wp_tempnam() reaches wp_unique_filename(), which reads the upload options; there is no database, so every option
// is answered here (an empty string for the ones these tests never look at).
add_filter('pre_option', static function ($pre, $option)
{
    $options = [
        'siteurl'     => 'http://example.org',
        'home'        => 'http://example.org',
        'upload_path' => WP_TEMP_DIR,
    ];
    return $options[$option] ?? '';
}, 10, 2);
