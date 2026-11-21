<?php
/**
 * Replays cases.json through whatever UTF-8 functions are installed and returns the mismatches (at most 20).
 *
 * Expects SPEEDYSPEC_UTF8_DIR set to this directory, and _deprecated_function() recording as record.php's does.
 */

require SPEEDYSPEC_UTF8_DIR . '/calls.php';
$speedyspec_recorded = json_decode(file_get_contents(SPEEDYSPEC_UTF8_DIR . '/cases.json'), true);
$speedyspec_mismatches = array();
foreach ($speedyspec_recorded['cases'] as $case) {
    $got = speedyspec_utf8_calls(base64_decode($case['input']));
    foreach ($case['calls'] as $name => $want) {
        if (($got[$name] ?? null) !== $want && count($speedyspec_mismatches) < 20) {
            $speedyspec_mismatches[] = sprintf('%s(%s): want %s got %s', $name, bin2hex(base64_decode($case['input'])), base64_decode($want), base64_decode($got[$name] ?? ''));
        }
    }
}
return array('cases' => count($speedyspec_recorded['cases']), 'mismatches' => $speedyspec_mismatches);
