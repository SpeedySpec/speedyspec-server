<?php

declare(strict_types=1);

/**
 * Loads what WordPress's translation tests need around the native translation functions: the stub test case, then
 *  the prepared release's l10n files (compat/i18n/load.php), the locale classes, and the few theme functions
 *  just-in-time loading reaches.
 *
 *  The copied tests are WordPress 7.1.5's `tests/phpunit/tests/l10n/wpTranslationController.php` and
 *  `wpTranslations.php`, from wordpress-develop at tag 7.1.5, named after their classes and with static data
 *  providers for the pinned PHPUnit 11; phase 3 added the script translation, schema, list separator, word count,
 *  WP_Locale and locale switcher tests. Users, the admin screen, themes and option writes are in-memory stand-ins
 *  (includes/i18n-doubles.php).
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

require __DIR__ . '/includes/i18n-doubles.php';

require dirname(__DIR__, 2) . '/compat/i18n/load.php';
require_once SPEEDYSPEC_I18N_ROOT . '/wp-includes/formatting.php';
require_once SPEEDYSPEC_I18N_ROOT . '/wp-includes/pomo/po.php';
// GlotPress's locales, which WordPress's pluralForms test would otherwise download.
require_once __DIR__ . '/includes/glotpress-locales.php';
do_action('after_setup_theme');
// WordPress's tests run after `init`, so enqueueing scripts raises no incorrect-usage notice.
do_action('init');
