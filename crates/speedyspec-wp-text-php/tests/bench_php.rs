//! The native formatting functions against WordPress 7.1.5's own formatting.php in the same embedded PHP, on the
//! corpus's inputs, called from PHP as WordPress calls them. Run on demand:
//! `SPEEDYSPEC_TEXT_BENCH=1 cargo test --release -p speedyspec-wp-text-php --test bench_php`.
//!
//! The release's formatting.php is loaded a second time inside `namespace Legacy`, its string callbacks pointed at
//! the `Legacy\` functions, so both run side by side over the same globals, settings and hooks.
//!
//! Gates: every function at least as fast as PHP on every input group, less a 5% margin for noise on a loaded
//! machine (0.95x); two documented exceptions, `wp_rel_nofollow` (0.9x: plan 4 measured it level with PHP, its time is
//! slashing the whole text) and `esc_textarea` on short inputs (0.8x: WordPress's own body calls `get_option()` on
//! every call, which the native must make through PHP, and on a few bytes that call costs more than the escaping);
//! and the `the_content` chain at least 3.5x. The chain's 5x target is reported.

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
    for function in FUNCTIONS {
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

    // the_content's formatting chain, as default-filters.php registers it, natives against legacy.
    speedyspec_php::eval(
        "chain",
        r#"foreach (['' => 'bench_native', 'Legacy\\' => 'bench_legacy'] as $ns => $hook) {
            add_filter($hook, $ns . 'wptexturize');
            add_filter($hook, $ns . 'wpautop');
            add_filter($hook, $ns . 'shortcode_unautop');
            add_filter($hook, $ns . 'capital_P_dangit', 11);
            add_filter($hook, $ns . 'convert_smilies', 20);
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
        let bytes: usize = docs.iter().map(Vec::len).sum();
        let ratio = rate(native, bytes) / rate(legacy, bytes);
        if ratio < CHAIN_FLOOR {
            missed.push(format!("the_content/{group} {ratio:.2}x"));
        }
        println!(
            "{:<28} {group:<12} {:>13} {:>13} {ratio:>6.2}x {:>6}  (target {CHAIN_TARGET}x: {})",
            "the_content chain",
            "",
            "",
            if ratio >= CHAIN_FLOOR { "yes" } else { "NO" },
            if ratio >= CHAIN_TARGET { "met" } else { "not met" }
        );
    }
    if !missed.is_empty() {
        eprintln!("FAIL: under the gate: {}", missed.join(", "));
        std::process::exit(1);
    }
}
