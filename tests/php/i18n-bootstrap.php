<?php

declare(strict_types=1);

/**
 * Loads what WordPress's translation tests need around the native translation functions: the stub test case, then
 *  the prepared release's l10n files (compat/i18n/load.php), the locale classes, and the few theme functions
 *  just-in-time loading reaches.
 *
 *  The copied tests are WordPress 7.1.5's `tests/phpunit/tests/l10n/wpTranslationController.php` and
 *  `wpTranslations.php`, from wordpress-develop at tag 7.1.5, named after their classes and with static data
 *  providers for the pinned PHPUnit 11. The other l10n tests need users, options or themes from a database install
 *  (loadTextdomain.php creates a user before its first test) and are not run here.
 */

require __DIR__ . '/bootstrap.php';

$speedyspec_root = getenv('SPEEDYSPEC_I18N_WP_ROOT');
if (!$speedyspec_root)
{
    throw new RuntimeException('Set SPEEDYSPEC_I18N_WP_ROOT to the prepared WordPress release.');
}

define('DIR_TESTDATA', __DIR__ . '/data');
define('WP_LANG_DIR', DIR_TESTDATA . '/languages');
define('WP_PLUGIN_DIR', DIR_TESTDATA . '/plugins');
define('SPEEDYSPEC_I18N_ROOT', $speedyspec_root);
define('SPEEDYSPEC_I18N_DATA', DIR_TESTDATA);

function _doing_it_wrong($function_name, $message, $version): void
{
    do_action('doing_it_wrong_run', $function_name, $message, $version);
}

function get_template_directory(): string
{
    return DIR_TESTDATA . '/themedir1/default';
}

function get_stylesheet_directory(): string
{
    return get_template_directory();
}

require dirname(__DIR__, 2) . '/compat/i18n/load.php';
require_once SPEEDYSPEC_I18N_ROOT . '/wp-includes/formatting.php';
require_once SPEEDYSPEC_I18N_ROOT . '/wp-includes/class-wp-locale.php';
require_once SPEEDYSPEC_I18N_ROOT . '/wp-includes/class-wp-locale-switcher.php';
$GLOBALS['wp_locale']          = new WP_Locale();
$GLOBALS['wp_locale_switcher'] = new WP_Locale_Switcher();
$GLOBALS['wp_locale_switcher']->init();
do_action('after_setup_theme');
