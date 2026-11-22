//! The native formatting functions against WordPress 7.1.5's own formatting.php in the same embedded PHP, on the
//! corpus's inputs, called from PHP as WordPress calls them. Run on demand:
//! `SPEEDYSPEC_TEXT_BENCH=1 cargo test --release -p speedyspec-wp-text-php --test bench_php` (add
//! `SPEEDYSPEC_TEXT_BENCH_ONLY=<function>` to time one function).
//!
//! The release's formatting.php is loaded a second time inside `namespace Legacy`, its string callbacks pointed at
//! the `Legacy\` functions, so both run side by side over the same globals, settings and hooks.
//!
//! Gates: every function at least as fast as PHP on every input group, less a 5% margin for noise on a loaded
//! machine (0.95x); three documented exceptions, `wp_rel_nofollow` (0.9x: plan 4 measured it level with PHP, its time is
//! slashing the whole text) and `esc_textarea` on short inputs (0.8x: WordPress's own body calls `get_option()` on
//! every call, which the native must make through PHP, and on a few bytes that call costs more than the escaping);
//! `wp_strip_all_tags` on short inputs (0.65x: the fast path's check that its handler hasn't been rebound costs
//! about a fifth of a call on a few bytes);
//! and the `the_content` chain at least 3.5x. The chain's 5x target is reported.
//!
//! Formatting's callbacks added by name run as Rust callbacks. The chain, and a `pre_user_url`-style pair
//! (`wp_strip_all_tags` then `sanitize_url`) on short URLs, are also timed against the same callbacks registered
//! through `WP_Hook::add_filter()`, which keeps them PHP callables: the swap must reach 0.95x of that or better.
//! `SPEEDYSPEC_TEXT_BENCH_ONLY=chain` times only the chains and the pair.

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, NativeArea, Php};
use speedyspec_php_ext::{Key, Value, twins_area};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::WpHooksArea;
use speedyspec_wp_kses_php::KsesArea;
use speedyspec_wp_text_corpus::{inputs, page};
use speedyspec_wp_text_php::{TextArea, prepare_release};

/// The chain's floor and its target (spec).
const CHAIN_FLOOR: f64 = 3.5;
const CHAIN_TARGET: f64 = 5.0;
/// Rust callbacks against the same callbacks kept as PHP callables: never slower, less noise.
const SWAP_FLOOR: f64 = 0.95;

/// The functions timed, and the corpus function whose inputs each uses.
const FUNCTIONS: &[&str] = &[
    "esc_html",
    "esc_attr",
    "esc_textarea",
    "esc_url",
    "sanitize_key",
    "sanitize_title",
    "sanitize_title_with_dashes",
    "sanitize_text_field",
    "sanitize_file_name",
    "sanitize_html_class",
    "is_email",
    "sanitize_email",
    "remove_accents",
    "wptexturize",
    "wpautop",
    "shortcode_unautop",
    "capital_P_dangit",
    "convert_chars",
    "force_balance_tags",
    "wp_strip_all_tags",
    "convert_smilies",
    "make_clickable",
    "wp_rel_nofollow",
];

/// Every function formatting.php declares (each gets a `Legacy\` twin, and its string callbacks are rewritten).
fn declared(source: &str) -> Vec<String> {
    source
        .lines()
        .filter_map(|l| l.strip_prefix("function "))
        .filter_map(|rest| rest.split('(').next())
        .map(|name| name.trim().to_string())
        .collect()
}

/// The release's formatting.php as PHP code in `namespace Legacy`.
fn legacy_formatting(source: &str) -> String {
    let mut code = source.trim_start_matches("<?php").to_string();
    // Names also used as hook names (`apply_filters( 'sanitize_title', ... )`) stay as they are.
    let hooks: Vec<String> = source
        .match_indices("apply_filters(")
        .filter_map(|(at, _)| source[at + "apply_filters(".len()..].trim_start().strip_prefix('\''))
        .filter_map(|rest| rest.split('\'').next())
        .map(str::to_string)
        .collect();
    for name in declared(source).into_iter().filter(|n| !hooks.contains(n)) {
        code = code.replace(&format!("'{name}'"), &format!("'Legacy\\\\{name}'"));
    }
    format!("namespace Legacy; use WP_HTML_Tag_Processor; use WP_Http; {code}\nreturn true;")
}

/// The best of three time-boxed runs of `call` over the inputs in `$GLOBALS['bench_docs']`: bytes per second.
fn rate(call: &str, bytes: usize) -> f64 {
    let code = format!(
        "$docs = $GLOBALS['bench_docs']; $n = 0; $t = hrtime(true);
        do {{ foreach ($docs as $d) {{ {call}; }} $n++; }} while (hrtime(true) - $t < 300000000);
        return [$n, (hrtime(true) - $t) / 1e9];"
    );
    (0..3)
        .map(|_| match speedyspec_php::eval("bench", &code) {
            Ok(Value::Array(items)) => {
                let n = match &items[0].1 {
                    Value::Int(n) => *n as f64,
                    other => panic!("{other:?}"),
                };
                let s = match &items[1].1 {
                    Value::Float(s) => *s,
                    other => panic!("{other:?}"),
                };
                bytes as f64 * n / s
            }
            other => panic!("{other:?}"),
        })
        .fold(0.0, f64::max)
}

/// `a` against `b` over the inputs in `$GLOBALS['bench_docs']`, timed in alternating 0.1-second rounds (eight of
/// each) so both see the same machine load: the best rate of each, in bytes per second.
fn rate_pair(a: &str, b: &str, bytes: usize) -> (f64, f64) {
    let round = |call: &str| {
        let code = format!(
            "$docs = $GLOBALS['bench_docs']; $n = 0; $t = hrtime(true);
            do {{ foreach ($docs as $d) {{ {call}; }} $n++; }} while (hrtime(true) - $t < 100000000);
            return [$n, (hrtime(true) - $t) / 1e9];"
        );
        match speedyspec_php::eval("bench", &code) {
            Ok(Value::Array(items)) => match (&items[0].1, &items[1].1) {
                (Value::Int(n), Value::Float(s)) => bytes as f64 * *n as f64 / s,
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        }
    };
    (0..8).fold((0.0, 0.0), |(best_a, best_b), _| (f64::max(best_a, round(a)), f64::max(best_b, round(b))))
}

fn set_docs(docs: &[Vec<u8>]) {
    let list = Value::Array(docs.iter().enumerate().map(|(i, d)| (Key::Int(i as i64), Value::Bytes(d.clone()))).collect());
    speedyspec_php::set_global("bench_docs", &list).unwrap();
}

/// Whether native and legacy agree on every input; on the first difference, the input and both results.
fn agree(native: &str, legacy: &str) -> Result<(), String> {
    let code = format!(
        "foreach ($GLOBALS['bench_docs'] as $d) {{ $n = {native}; $l = {legacy}; if ($n !== $l) {{ return var_export([$d, $n, $l], true); }} }} return true;"
    );
    match speedyspec_php::eval("agree", &code) {
        Ok(Value::Bool(true)) => Ok(()),
        other => Err(format!("{other:?}")),
    }
}

fn main() {
    if std::env::var_os("SPEEDYSPEC_TEXT_BENCH").is_none() {
        println!("skipped: set SPEEDYSPEC_TEXT_BENCH=1 to run the PHP benchmark");
        return;
    }
    let release = speedyspec_compat::wordpress_release("7.1.5").expect("the pinned release");
    let out = tempfile::tempdir().unwrap();
    let root = prepare_release(&release, &out.path().join("wordpress")).unwrap();
    let container = Rc::new(Container::new());
    let hooks = WpHooksArea::new(container.clone(), Rc::new(MemoryHookStore::new()));
    let kses = KsesArea::new(container.clone(), hooks.api());
    let text = TextArea::new(container.clone(), hooks.api(), kses.api());
    let areas: Vec<Rc<dyn NativeArea>> =
        vec![twins_area(), Rc::new(ContainerArea::new("bench", container)), Rc::new(hooks), Rc::new(kses), Rc::new(text)];
    let php = Php::boot(BootOptions { hooks: Rc::new(MemoryHooks::new()), preload: &[], areas }).expect("PHP boots");
    php.restart_request().unwrap();
    let setup = format!(
        "define('SPEEDYSPEC_COMPAT_WP_ROOT', {:?}); define('SPEEDYSPEC_COMPAT_PORT', true); require {:?}; return true;",
        format!("{}/", root.display()),
        speedyspec_compat::compat_dir().join("formatting/setup.php").display().to_string()
    );
    speedyspec_php::eval("setup", &setup).expect("the release loads");
    let source = std::fs::read_to_string(release.join("wp-includes/formatting.php")).unwrap();
    speedyspec_php::eval("legacy", &legacy_formatting(&source)).expect("legacy formatting loads");

    println!("{:<28} {:<12} {:>13} {:>13} {:>7} {:>6}", "function", "group", "native MB/s", "legacy MB/s", "ratio", "met");
    let mut missed = Vec::new();
    // SPEEDYSPEC_TEXT_BENCH_ONLY=name times one function (and skips the chain).
    let only = std::env::var("SPEEDYSPEC_TEXT_BENCH_ONLY").ok();
    for function in FUNCTIONS.iter().filter(|f| only.as_deref().is_none_or(|o| o == **f)) {
        let posts: Vec<Vec<u8>> =
            inputs(function).into_iter().filter(|(s, _)| s.contains("/whole_posts/")).map(|(_, b)| b).collect();
        let mut groups: Vec<(&str, Vec<Vec<u8>>)> = Vec::new();
        // The short inputs, less the crafted PCRE backtrack cases: they measure the documented backtrack-limit break
        // (PHP gives up where the port finishes), not throughput.
        let short: Vec<Vec<u8>> = inputs(function)
            .into_iter()
            .filter(|(s, _)| !s.contains("/whole_posts/") && !s.contains("backtrack"))
            .map(|(_, b)| b)
            .collect();
        if !short.is_empty() {
            groups.push(("short", short));
        }
        if !posts.is_empty() {
            groups.push(("whole_posts", posts));
            groups.push(("page", vec![page(function)]));
        }
        for (group, docs) in groups {
            let floor = match (*function, group) {
                ("wp_rel_nofollow", _) => 0.9,
                ("esc_textarea", "short") => 0.8,
                ("wp_strip_all_tags", "short") => 0.65,
                _ => 0.95,
            };
            set_docs(&docs);
            let (native, legacy) = (format!("{function}($d)"), format!("Legacy\\{function}($d)"));
            if let Err(why) = agree(&native, &legacy) {
                panic!("native and legacy {function} differ on {group}: {why}");
            }
            let bytes: usize = docs.iter().map(Vec::len).sum();
            let (n, l) = (rate(&native, bytes), rate(&legacy, bytes));
            let ratio = n / l;
            let met = ratio >= floor;
            if !met {
                missed.push(format!("{function}/{group} {ratio:.2}x"));
            }
            println!(
                "{function:<28} {group:<12} {:>13.1} {:>13.1} {ratio:>6.2}x {:>6}",
                n / 1e6 * 0.953_674_3,
                l / 1e6 * 0.953_674_3,
                if met { "yes" } else { "NO" }
            );
        }
    }

    if only.as_deref().is_some_and(|o| o != "chain") {
        if !missed.is_empty() {
            eprintln!("FAIL: under the gate: {}", missed.join(", "));
            std::process::exit(1);
        }
        return;
    }
    // the_content's formatting chain, as default-filters.php registers it: natives added by name (Rust callbacks),
    // the same natives through WP_Hook::add_filter() (PHP callables), and legacy.
    speedyspec_php::eval(
        "chain",
        r#"$GLOBALS['wp_filter']['bench_unswapped'] = new WP_Hook();
        $GLOBALS['wp_filter']['bench_pair_unswapped'] = new WP_Hook();
        $chain = [['wptexturize', 10], ['wpautop', 10], ['shortcode_unautop', 10], ['capital_P_dangit', 11], ['convert_smilies', 20]];
        foreach ($chain as [$f, $p]) {
            add_filter('bench_native', $f, $p);
            add_filter('bench_legacy', 'Legacy\\' . $f, $p);
            $GLOBALS['wp_filter']['bench_unswapped']->add_filter('bench_unswapped', $f, $p, 1);
        }
        foreach (['wp_strip_all_tags', 'sanitize_url'] as $f) {
            add_filter('bench_pair', $f);
            $GLOBALS['wp_filter']['bench_pair_unswapped']->add_filter('bench_pair_unswapped', $f, 10, 1);
        }
        return true;"#,
    )
    .unwrap();
    let posts: Vec<Vec<u8>> =
        inputs("wptexturize").into_iter().filter(|(s, _)| s.contains("/whole_posts/")).map(|(_, b)| b).collect();
    for (group, docs) in [("whole_posts", posts), ("page", vec![page("wptexturize")])] {
        set_docs(&docs);
        let (native, legacy) = ("apply_filters('bench_native', $d)", "apply_filters('bench_legacy', $d)");
        if let Err(why) = agree(native, legacy) {
            panic!("the native and legacy chains differ on {group}: {why}");
        }
        let unswapped = "apply_filters('bench_unswapped', $d)";
        if let Err(why) = agree(native, unswapped) {
            panic!("the swapped and unswapped chains differ on {group}: {why}");
        }
        let bytes: usize = docs.iter().map(Vec::len).sum();
        let ratio = rate(native, bytes) / rate(legacy, bytes);
        let (n, u) = rate_pair(native, unswapped, bytes);
        let swap = n / u;
        if ratio < CHAIN_FLOOR {
            missed.push(format!("the_content/{group} {ratio:.2}x"));
        }
        if swap < SWAP_FLOOR {
            missed.push(format!("the_content/{group} swap {swap:.2}x"));
        }
        println!(
            "{:<28} {group:<12} {:>13} {:>13} {ratio:>6.2}x {:>6}  (target {CHAIN_TARGET}x: {}; swapped/unswapped {swap:.2}x)",
            "the_content chain",
            "",
            "",
            if ratio >= CHAIN_FLOOR && swap >= SWAP_FLOOR { "yes" } else { "NO" },
            if ratio >= CHAIN_TARGET { "met" } else { "not met" }
        );
    }
    // A pre_user_url-style pair on short URLs: where the crossing costs most against the work.
    let urls: Vec<Vec<u8>> = inputs("esc_url")
        .into_iter()
        .filter(|(s, _)| !s.contains("/whole_posts/") && !s.contains("backtrack"))
        .map(|(_, b)| b)
        .collect();
    set_docs(&urls);
    let (swapped, unswapped) = ("apply_filters('bench_pair', $d)", "apply_filters('bench_pair_unswapped', $d)");
    if let Err(why) = agree(swapped, unswapped) {
        panic!("the swapped and unswapped URL pairs differ: {why}");
    }
    let bytes: usize = urls.iter().map(Vec::len).sum();
    let (n, u) = rate_pair(swapped, unswapped, bytes);
    let swap = n / u;
    if swap < SWAP_FLOOR {
        missed.push(format!("url_pair swap {swap:.2}x"));
    }
    println!(
        "{:<28} {:<12} {:>13} {:>13} {swap:>6.2}x {:>6}  (swapped/unswapped)",
        "url pair",
        "short",
        "",
        "",
        if swap >= SWAP_FLOOR { "yes" } else { "NO" }
    );
    if !missed.is_empty() {
        eprintln!("FAIL: under the gate: {}", missed.join(", "));
        std::process::exit(1);
    }
}
