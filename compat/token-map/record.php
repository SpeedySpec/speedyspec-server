<?php
/**
 * Records WordPress's token map into cases.json.
 *
 * Usage: .php-build/buildroot/bin/php compat/token-map/record.php <wordpress-root>
 */

[$_, $root] = $argv + [null, null];
define('SPEEDYSPEC_TOKEN_MAP_ROOT', $root);
require __DIR__ . '/load.php';
require __DIR__ . '/calls.php';
$calls = speedyspec_token_map_calls(dirname(__DIR__, 2) . '/tests/php/data/html5-entities/entities.json');
file_put_contents(__DIR__ . '/cases.json', json_encode($calls, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES) . "\n");
echo count($calls), " calls recorded\n";
