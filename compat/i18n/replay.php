<?php
/**
 * Replays the corpus through whatever translation functions are installed and returns the mismatches (at most 30).
 * Expects SPEEDYSPEC_I18N_ROOT and SPEEDYSPEC_I18N_DATA defined, and load.php already required.
 */

require __DIR__ . '/calls.php';
/** The first places two values differ, as `path: want … got …` lines. */
function speedyspec_i18n_diff($want, $got, string $path = ''): string {
    if (is_array($want) && is_array($got)) {
        $out = array();
        foreach (array_unique(array_merge(array_keys($want), array_keys($got))) as $k) {
            if (!array_key_exists($k, $got) || !array_key_exists($k, $want) || $want[$k] !== $got[$k]) {
                $out[] = speedyspec_i18n_diff($want[$k] ?? '(missing)', $got[$k] ?? '(missing)', "$path/$k");
            }
        }
        return implode('; ', array_slice($out, 0, 8));
    }
    return "$path want " . var_export($want, true) . ' got ' . var_export($got, true);
}

$speedyspec_want = json_decode(file_get_contents(__DIR__ . '/cases.json'), true);
$speedyspec_got  = speedyspec_i18n_calls(SPEEDYSPEC_I18N_DATA);
$speedyspec_mismatches = array();
foreach ($speedyspec_want as $name => $want) {
    $got = $speedyspec_got[$name] ?? '(missing)';
    if ($got !== $want && count($speedyspec_mismatches) < 30) {
        $decode = function ($v) { return 0 === strpos($v, 'threw') ? $v : @unserialize(base64_decode($v)); };
        $speedyspec_mismatches[] = "$name: " . speedyspec_i18n_diff($decode($want), $decode($got));
    }
}
return array('calls' => count($speedyspec_want), 'mismatches' => $speedyspec_mismatches);
