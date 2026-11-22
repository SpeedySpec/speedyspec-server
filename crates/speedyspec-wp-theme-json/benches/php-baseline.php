<?php
/**
 * The same operations as `stylesheet.rs`, timed in WordPress 7.1.5's own PHP classes, for docs/benchmarks.md.
 *
 * Run: `php benches/php-baseline.php <wordpress 7.1.5 root>` from this crate's directory. It loads the classes the
 * way the compat recorder does (compat/theme-json/setup.php) and registers every core block.
 */

ini_set('display_errors', 'stderr');
define('SPEEDYSPEC_COMPAT_WP_ROOT', rtrim($argv[1] ?? '', '/') . '/');
define('SPEEDYSPEC_COMPAT_PORT', false);
define('SPEEDYSPEC_COMPAT_MULTISITE', false);
require __DIR__ . '/../../../compat/theme-json/setup.php';
compat_register_core_blocks();

/** Median microseconds per call over enough calls to fill about half a second. */
function speedyspec_bench(string $name, callable $fn): void
{
    $fn();
    $times = array();
    $deadline = hrtime(true) + 500000000;
    do {
        $start = hrtime(true);
        $fn();
        $times[] = (hrtime(true) - $start) / 1000;
    } while (hrtime(true) < $deadline || count($times) < 10);
    sort($times);
    printf("%-45s %10.1f µs\n", $name, $times[intdiv(count($times), 2)]);
}

$core = json_decode(file_get_contents(__DIR__ . '/data/core.json'), true);
$core_doc = new WP_Theme_JSON($core, 'default');
foreach (array('twentytwentythree', 'twentytwentyfour', 'twentytwentyfive') as $theme) {
    $raw = json_decode(file_get_contents(__DIR__ . "/data/$theme.json"), true);
    speedyspec_bench("$theme/construct", fn() => new WP_Theme_JSON($raw, 'theme'));
    $theme_doc = new WP_Theme_JSON($raw, 'theme');
    speedyspec_bench("$theme/merge_into_core", function () use ($core_doc, $theme_doc) {
        $merged = clone $core_doc;
        $merged->merge($theme_doc);
    });
    $merged = clone $core_doc;
    $merged->merge($theme_doc);
    foreach (array('variables', 'styles', 'presets', 'custom-css') as $type) {
        speedyspec_bench("$theme/get_stylesheet/$type", fn() => $merged->get_stylesheet(array($type), null, array('include_block_style_variations' => true)));
    }
}
speedyspec_bench('core/construct', fn() => new WP_Theme_JSON($core, 'default'));
speedyspec_bench('core/get_stylesheet/default', fn() => $core_doc->get_stylesheet(array('variables', 'styles', 'presets'), null, array('include_block_style_variations' => true)));
