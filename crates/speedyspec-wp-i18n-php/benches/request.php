<?php
/**
 * The request benchmark's workload, run the same way against stock WordPress (static PHP CLI) and the port
 * (embedded PHP): per simulated request, load core de_DE (default, admin) and WooCommerce de_DE, then make 4000
 * translation calls (55% default, 45% woocommerce, 25% untranslated, seed 42) twice. The first pass includes the
 * lazy parse of the files; the second is the steady cost per call.
 *
 * Expects compat/i18n/load.php loaded (SPEEDYSPEC_I18N_ROOT, SPEEDYSPEC_I18N_DATA).
 */

function speedyspec_bench_work(string $packs): array {
    $keys = array();
    foreach (array('default' => "$packs/core/de_DE.l10n.php", 'woocommerce' => "$packs/woo/woocommerce-de_DE.l10n.php") as $domain => $file) {
        $data = include $file;
        foreach (array_keys($data['messages']) as $k) {
            $k = (string) $k;
            if (!str_contains($k, "\4")) {
                $keys[$domain][] = $k;
            }
        }
    }
    mt_srand(42);
    $work = array();
    for ($i = 0; $i < 4000; $i++) {
        $domain = mt_rand(0, 99) < 55 ? 'default' : 'woocommerce';
        if (mt_rand(0, 99) < 25) {
            $work[] = array($domain, 'Untranslated string number ' . mt_rand(0, 500));
        } else {
            $list   = $keys[$domain];
            $work[] = array($domain, $list[mt_rand(0, count($list) - 1)]);
        }
    }
    return $work;
}

function speedyspec_bench_reset(): void {
    $GLOBALS['l10n']          = array();
    $GLOBALS['l10n_unloaded'] = array();
    $r = new ReflectionProperty(WP_Translation_Controller::class, 'instance');
    $r->setValue(null, null);
    $GLOBALS['wp_textdomain_registry'] = new WP_Textdomain_Registry();
}

/**
 * Runs one case: `$format` is 'php' (WordPress's default, .l10n.php first) or 'mo'; `$case` is 'plain', 'filter'
 * (a closure on gettext) or 'object' (a Translations subclass in $l10n['woocommerce']).
 */
function speedyspec_bench_case(string $packs, string $format, string $case, int $requests = 200): array {
    $work  = speedyspec_bench_work($packs);
    $files = array('default' => "$packs/core/de_DE.mo", 'admin' => "$packs/core/admin-de_DE.mo", 'woocommerce' => "$packs/woo/woocommerce-de_DE.mo");
    $format_filter = function () use ($format) { return $format; };
    add_filter('translation_file_format', $format_filter);
    $gettext = function ($t) { return $t; };
    if ('filter' === $case) {
        add_filter('gettext', $gettext);
    }
    $first = array();
    $steady = array();
    $sum = 0;
    for ($r = 0; $r < $requests; $r++) {
        speedyspec_bench_reset();
        $t0 = hrtime(true);
        foreach ($files as $domain => $file) {
            load_textdomain($domain, $file, 'de_DE');
        }
        if ('object' === $case) {
            $GLOBALS['l10n']['woocommerce'] = new class extends Translations {
                public function translate($singular, $context = null) { return $singular; }
            };
        }
        foreach ($work as $w) {
            $sum += strlen(__($w[1], $w[0]));
        }
        $t1 = hrtime(true);
        foreach ($work as $w) {
            $sum += strlen(__($w[1], $w[0]));
        }
        $t2 = hrtime(true);
        if ($r >= 5) {
            $first[]  = $t1 - $t0;
            $steady[] = $t2 - $t1;
        }
    }
    $stats = function_exists('SpeedySpec\\WpI18n\\Native\\hot_stats') ? \SpeedySpec\WpI18n\Native\hot_stats() : null;
    remove_filter('translation_file_format', $format_filter);
    remove_filter('gettext', $gettext);
    sort($first);
    sort($steady);
    $median = function ($a) { return $a[intdiv(count($a), 2)]; };
    return array('request_us' => $median($first) / 1000, 'ns_per_call' => $median($steady) / count($work), 'check' => $sum / $requests, 'stats' => $stats);
}

function speedyspec_bench_run(string $packs): array {
    $out = array();
    foreach (array('php', 'mo') as $format) {
        foreach (array('plain', 'filter', 'object') as $case) {
            $out["$format/$case"] = speedyspec_bench_case($packs, $format, $case);
        }
    }
    $out['opcache'] = function_exists('opcache_get_status') && is_array(opcache_get_status(false)) ? (opcache_get_status(false)['jit']['on'] ?? false) : false;
    return $out;
}
