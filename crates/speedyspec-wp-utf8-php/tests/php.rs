//! The UTF-8 functions in embedded PHP: the facade over the natives, and every case WordPress recorded
//! (`compat/utf8/cases.json`) replayed through it.
//!
//! PHP boots once per process, so this test has its own `main()` (`harness = false`).

use std::path::Path;
use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, NativeArea, Php, PhpError};
use speedyspec_wp_hooks_contracts::HookValue;
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::WpHooksArea;
use speedyspec_wp_utf8_php::{Utf8Area, prepare_release};

fn check(name: &str, ok: bool, detail: impl std::fmt::Debug) {
    if ok {
        println!("ok: {name}");
    } else {
        eprintln!("FAIL: {name}: {detail:?}");
        std::process::exit(1);
    }
}

fn text(result: &Result<HookValue, PhpError>) -> String {
    match result {
        Ok(HookValue::Str(s)) => s.clone(),
        other => format!("{other:?}"),
    }
}

fn main() {
    let release = speedyspec_compat::wordpress_release(&speedyspec_compat::pinned_wordpress())
        .expect("the pinned release");
    let prepared = prepare_release(&release, &release.with_file_name("wordpress-utf8-port"))
        .expect("release prepared");
    let container = Rc::new(Container::new());
    let hooks = WpHooksArea::new(container, Rc::new(MemoryHookStore::new()));
    let areas: Vec<Rc<dyn NativeArea>> = vec![Rc::new(hooks), Rc::new(Utf8Area)];
    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    })
    .expect("PHP boots");

    let load = format!(
        "error_reporting(E_ALL & ~E_DEPRECATED);
         function _deprecated_function($function, $version, $replacement = '') {{ $GLOBALS['speedyspec_deprecated'][] = array($function, $version, $replacement); }}
         foreach (array('compat.php', 'compat-utf8.php', 'utf8.php') as $f) {{ require {:?} . 'wp-includes/' . $f; }}",
        format!("{}/", prepared.display())
    );
    php.eval("speedyspec://test/load", &load)
        .expect("the prepared release's files load");

    let r = php.eval(
        "speedyspec://test/facade",
        "return (new ReflectionFunction('_wp_scan_utf8'))->getFileName();",
    );
    check(
        "functions_are_the_facade",
        text(&r) == "speedyspec://wp-utf8",
        &r,
    );

    let r = php.eval(
        "speedyspec://test/basic",
        "return bin2hex(serialize(array(wp_is_valid_utf8(\"a\\xFF\"), wp_scrub_utf8(\"\\xC0\\xAF\"), wp_has_noncharacters(\"\\xEF\\xB7\\x90\\xFF\"))));",
    );
    let want = "613a333a7b693a303b623a303b693a313b733a363a22efbfbdefbfbd223b693a323b623a313b7d";
    check("natives_are_registered_and_match", text(&r) == want, &r);

    let r = php.eval(
        "speedyspec://test/refs",
        "$at = 0; $len = 0; $nc = null; $n = _wp_scan_utf8(\"a\\xC3\\xA9\\xFF\", $at, $len, null, null, $nc);
         $found = 0; $bytes = _wp_utf8_codepoint_span(\"a\\xC3\\xA9b\", 0, 2, $found);
         return var_export(array($n, $at, $len, $nc, $bytes, $found), true);",
    );
    check(
        "scan_writes_references",
        text(&r)
            == "array (\n  0 => 2,\n  1 => 3,\n  2 => 1,\n  3 => false,\n  4 => 3,\n  5 => 2,\n)",
        &r,
    );

    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../compat/utf8");
    let code = format!(
        "define('SPEEDYSPEC_UTF8_DIR', {:?}); $r = require SPEEDYSPEC_UTF8_DIR . '/replay.php'; return json_encode($r);",
        dir.display().to_string()
    );
    let r = php.eval("speedyspec://test/replay", &code);
    let report: serde_json::Value =
        serde_json::from_str(&text(&r)).unwrap_or(serde_json::Value::Null);
    check(
        "recorded_cases_replay",
        report["cases"].as_i64().unwrap_or(0) > 600
            && report["mismatches"]
                .as_array()
                .is_some_and(|m| m.is_empty()),
        &report,
    );
}
