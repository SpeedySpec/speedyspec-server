//! The native `wp_kses_post()` against the pinned WordPress release's own kses.php in the same embedded PHP (goal: at
//! least 5x), on the benchmark corpus. Run on demand: `SPEEDYSPEC_KSES_BENCH=1 cargo test --release -p speedyspec-wp-kses-php
//! --test bench_php`.
//!
//! The release's kses.php is loaded a second time inside `namespace Legacy`, its string callbacks pointed at the
//! `Legacy\` functions, so both run side by side over the same globals and hooks.

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, NativeArea, Php};
use speedyspec_php_ext::{Key, Value, twins_area};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::WpHooksArea;
use speedyspec_wp_kses_php::{KsesArea, PORTED_FUNCTIONS, prepare_release};
use speedyspec_wp_kses_testkit::{bench_page, bench_snippets};

const GOAL: f64 = 5.0;

/// The release's kses.php as PHP code in `namespace Legacy`, its own functions' string callbacks namespaced too.
fn legacy_kses(source: &str) -> String {
    let mut code = source.trim_start_matches("<?php").to_string();
    for name in PORTED_FUNCTIONS {
        code = code.replace(&format!("'{name}'"), &format!("'Legacy\\\\{name}'"));
    }
    format!(
        "namespace Legacy; use WP_HTML_Tag_Processor; use WP_HTML_Decoder; {code}\nreturn true;"
    )
}

fn seconds(code: &str) -> f64 {
    match speedyspec_php::eval("bench", code) {
        Ok(Value::Float(f)) => f,
        other => panic!("{other:?}"),
    }
}

fn main() {
    if std::env::var_os("SPEEDYSPEC_KSES_BENCH").is_none() {
        println!("skipped: set SPEEDYSPEC_KSES_BENCH=1 to run the PHP benchmark");
        return;
    }
    let release = speedyspec_compat::wordpress_release(&speedyspec_compat::area_wordpress("kses"))
        .expect("the pinned release");
    let out = tempfile::tempdir().unwrap();
    let root = prepare_release(&release, &out.path().join("wordpress")).unwrap();
    let container = Rc::new(Container::new());
    let hooks = WpHooksArea::new(container.clone(), Rc::new(MemoryHookStore::new()));
    let kses = KsesArea::new(container.clone(), hooks.api());
    let areas: Vec<Rc<dyn NativeArea>> = vec![
        twins_area(),
        Rc::new(ContainerArea::new("bench", container)),
        Rc::new(hooks),
        Rc::new(kses),
    ];
    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    })
    .expect("PHP boots");
    php.restart_request().unwrap();
    let setup = format!(
        "define('ABSPATH', {:?}); define('WPINC', 'wp-includes');
        function current_user_can($capability, ...$args) {{ return false; }}
        add_filter('pre_option', function () {{ return ''; }});
        add_filter('pre_wp_load_alloptions', function () {{ return ['blog_charset' => 'UTF-8']; }});
        foreach (['load', 'compat-utf8', 'compat', 'utf8', 'class-wp-token-map', 'formatting', 'functions', 'kses'] as $f) {{
            require ABSPATH . WPINC . \"/$f.php\";
        }}
        foreach (['html5-named-character-references', 'class-wp-html-attribute-token', 'class-wp-html-span',
            'class-wp-html-doctype-info', 'class-wp-html-text-replacement', 'class-wp-html-decoder',
            'class-wp-html-tag-processor'] as $f) {{
            require ABSPATH . WPINC . \"/html-api/$f.php\";
        }}
        return true;",
        format!("{}/", root.display())
    );
    speedyspec_php::eval("setup", &setup).expect("the release loads");
    let source = std::fs::read_to_string(release.join("wp-includes/kses.php")).unwrap();
    speedyspec_php::eval("legacy", &legacy_kses(&source)).expect("legacy kses loads");

    let iterations = std::env::var("KSES_BENCH_ITERATIONS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);
    let corpora = [("snippets", bench_snippets()), ("page", vec![bench_page()])];
    println!(
        "{:<10} {:>12} {:>12} {:>8} {:>6} {:>5}",
        "corpus", "native MB/s", "legacy MB/s", "ratio", "goal", "met"
    );
    let mut missed = false;
    for (name, docs) in corpora {
        let bytes: usize = docs.iter().map(Vec::len).sum();
        let list = Value::Array(
            docs.iter()
                .enumerate()
                .map(|(i, d)| (Key::Int(i as i64), Value::Bytes(d.clone())))
                .collect(),
        );
        speedyspec_php::set_global("bench_docs", &list).unwrap();
        // Both must agree before either is timed.
        let same = speedyspec_php::eval(
            "agree",
            "foreach ($GLOBALS['bench_docs'] as $d) { if (wp_kses_post($d) !== Legacy\\wp_kses_post($d)) { return false; } } return true;",
        );
        assert!(
            matches!(same, Ok(Value::Bool(true))),
            "native and legacy differ on {name}: {same:?}"
        );
        let timed = |f: &str| {
            seconds(&format!(
                "$docs = $GLOBALS['bench_docs']; $t = hrtime(true);
                for ($i = 0; $i < {iterations}; $i++) {{ foreach ($docs as $d) {{ {f}($d); }} }}
                return (hrtime(true) - $t) / 1e9;"
            ))
        };
        let native = (0..3)
            .map(|_| timed("wp_kses_post"))
            .fold(f64::MAX, f64::min);
        let legacy = (0..3)
            .map(|_| timed("Legacy\\wp_kses_post"))
            .fold(f64::MAX, f64::min);
        let mb = |s: f64| (bytes * iterations) as f64 / s / 1e6;
        let ratio = legacy / native;
        missed |= ratio < GOAL;
        println!(
            "{name:<10} {:>12.1} {:>12.1} {ratio:>8.1} {GOAL:>6.0} {:>5}",
            mb(native),
            mb(legacy),
            if ratio >= GOAL { "yes" } else { "NO" }
        );
    }
    if missed {
        std::process::exit(1);
    }
}
