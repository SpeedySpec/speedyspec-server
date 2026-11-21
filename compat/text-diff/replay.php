<?php
/**
 * Replays cases.json through whatever wp_text_diff() is installed, in the recorded order, and returns a report.
 *
 * Expects WordPress's functions loaded and SPEEDYSPEC_TEXT_DIFF_CASES set to the path of cases.json. The filter
 * cases install the same callbacks record.php did, and the run counts of process_text_diff_html must match too: they
 * accumulate over every case before them, so one extra or missing apply_filters() anywhere shows up.
 */

$speedyspec_cases = json_decode(file_get_contents(SPEEDYSPEC_TEXT_DIFF_CASES), true);
$speedyspec_decode = function ($v) {
    return is_array($v) ? base64_decode($v['b64']) : $v;
};
$speedyspec_filters = array(
    'wrap' => function ($processed, $line, $kind) { return "[$kind]$processed"; },
    'raw' => function ($processed, $line, $kind) { return $line; },
    'int' => function ($processed, $line, $kind) { return 7; },
);
$speedyspec_failures = array();
$speedyspec_ran = 0;
foreach ($speedyspec_cases as $case) {
    $left = $speedyspec_decode($case['left']);
    $right = $speedyspec_decode($case['right']);
    $args = $case['args'];
    if ($case['name'] === 'object_args') {
        $args = (object) $args;
    }
    if (isset($case['filter'])) {
        add_filter('process_text_diff_html', $speedyspec_filters[$case['filter']], 10, 3);
    }
    try {
        $output = wp_text_diff($left, $right, $args);
        if (isset($case['throws'])) {
            $speedyspec_failures[] = "{$case['name']}: expected {$case['throws']}, got output";
        } elseif ($output !== $speedyspec_decode($case['output'])) {
            $speedyspec_failures[] = "{$case['name']}: output differs\n  got      " . json_encode($output) . "\n  expected " . json_encode($speedyspec_decode($case['output']));
        }
    } catch (Throwable $t) {
        if (!isset($case['throws']) || !($t instanceof $case['throws'])) {
            $speedyspec_failures[] = "{$case['name']}: threw " . get_class($t) . ': ' . $t->getMessage();
        }
    }
    if (isset($case['filter'])) {
        if (did_filter('process_text_diff_html') !== $case['did_filter']) {
            $speedyspec_failures[] = "{$case['name']}: did_filter " . did_filter('process_text_diff_html') . ", expected {$case['did_filter']}";
        }
        remove_all_filters('process_text_diff_html');
    }
    $speedyspec_ran++;
}
return $speedyspec_ran . ' cases' . ($speedyspec_failures ? ', ' . count($speedyspec_failures) . " failed:\n" . implode("\n", array_slice($speedyspec_failures, 0, 5)) : ', all passed');
