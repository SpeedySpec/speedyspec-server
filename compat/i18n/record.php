<?php
/**
 * Records WordPress's translation functions over the fixtures into cases.json.
 *
 * Usage: .php-build/buildroot/bin/php compat/i18n/record.php <wordpress-root>
 */

[$_, $root] = $argv + [null, null];
define('SPEEDYSPEC_I18N_ROOT', $root);
define('SPEEDYSPEC_I18N_DATA', dirname(__DIR__, 2) . '/crates/speedyspec-wp-i18n-files/tests/data');
require __DIR__ . '/load.php';
require __DIR__ . '/calls.php';
$calls = speedyspec_i18n_calls(SPEEDYSPEC_I18N_DATA);
file_put_contents(__DIR__ . '/cases.json', json_encode($calls, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES) . "\n");
echo count($calls), " calls recorded\n";
