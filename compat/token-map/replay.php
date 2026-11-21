<?php
/**
 * Replays cases.json against the loaded token map (load.php has run); returns the count and the first mismatches.
 */

require __DIR__ . '/calls.php';

$speedyspec_want = json_decode(file_get_contents(__DIR__ . '/cases.json'), true);
$speedyspec_got = speedyspec_token_map_calls(dirname(__DIR__, 2) . '/tests/php/data/html5-entities/entities.json');
$speedyspec_mismatches = array();
foreach ($speedyspec_want as $name => $want) {
    $got = $speedyspec_got[$name] ?? '(missing)';
    if ($got !== $want && count($speedyspec_mismatches) < 20) {
        $speedyspec_mismatches[] = "$name: want " . substr(var_export(unserialize(base64_decode($want)), true), 0, 600)
            . ' got ' . substr(var_export(@unserialize(base64_decode($got)), true), 0, 600);
    }
}
return array('calls' => count($speedyspec_want), 'mismatches' => $speedyspec_mismatches);
