<?php
/**
 * Runs the request benchmark against stock WordPress on the static PHP CLI and prints JSON.
 *
 * Usage: php stock.php <wordpress-root> <packs>
 */

[$_, $root, $packs] = $argv;
define('SPEEDYSPEC_I18N_ROOT', $root);
define('SPEEDYSPEC_I18N_DATA', $packs);
require dirname(__DIR__, 3) . '/compat/i18n/load.php';
require __DIR__ . '/request.php';
echo json_encode(speedyspec_bench_run($packs));
