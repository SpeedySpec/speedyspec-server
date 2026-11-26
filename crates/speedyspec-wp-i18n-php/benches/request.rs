//! The request benchmark: WordPress translations through the port (embedded PHP) against stock WordPress on
//! the same static PHP (CLI), both with OPcache and the tracing JIT, over core de_DE and WooCommerce de_DE.
//!
//! Run `benches/fetch-packs.sh` first. The bar: with nothing hooked, a translated request at least 3x faster than
//! stock WordPress with `.l10n.php` files, and no case slower than stock.

use std::path::Path;
use std::process::Command;
use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, NativeArea, Php};
use speedyspec_wp_hooks::HookValue;
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::WpHooksArea;
use speedyspec_wp_i18n_php::{I18nArea, prepare_release};
use speedyspec_wp_utf8_php::Utf8Area;

const JIT: &[(&str, &str)] = &[
    ("opcache.enable", "1"),
    ("opcache.enable_cli", "1"),
    ("opcache.jit", "tracing"),
    ("opcache.jit_buffer_size", "64M"),
    ("memory_limit", "1G"),
];

fn main() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = crate_dir.join("../..");
    let packs = crate_dir.join("benches/packs");
    if !packs.join("core/de_DE.mo").exists() {
        eprintln!("run benches/fetch-packs.sh first");
        std::process::exit(2);
    }
    let release = speedyspec_compat::wordpress_release(&speedyspec_compat::pinned_wordpress())
        .expect("the pinned release");
    let prepared = prepare_release(&release, &release.with_file_name("wordpress-i18n-port"))
        .expect("prepared");

    // Stock WordPress on the static PHP CLI.
    let mut php_cli = Command::new(root.join(".php-build/buildroot/bin/php"));
    for (k, v) in JIT {
        php_cli.arg("-d").arg(format!("{k}={v}"));
    }
    let stock = php_cli
        .arg(crate_dir.join("benches/stock.php"))
        .arg(&release)
        .arg(&packs)
        .output()
        .expect("the static PHP runs");
    let stock: serde_json::Value = serde_json::from_slice(&stock.stdout)
        .unwrap_or_else(|e| panic!("stock run: {e}: {}", String::from_utf8_lossy(&stock.stderr)));

    // The port in embedded PHP.
    speedyspec_php::set_ini(
        &JIT.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect::<Vec<_>>(),
    )
    .expect("INI settings");
    let container = Rc::new(Container::new());
    let hooks = WpHooksArea::new(container.clone(), Rc::new(MemoryHookStore::new()));
    let i18n = I18nArea::new(container, hooks.api());
    let areas: Vec<Rc<dyn NativeArea>> = vec![Rc::new(hooks), Rc::new(Utf8Area), Rc::new(i18n)];
    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    })
    .expect("PHP boots");
    let code = format!(
        "define('SPEEDYSPEC_I18N_ROOT', {:?}); define('SPEEDYSPEC_I18N_DATA', {:?}); require {:?}; require {:?};
         return json_encode(speedyspec_bench_run({:?}));",
        prepared.display().to_string(),
        packs.display().to_string(),
        root.join("compat/i18n/load.php").display().to_string(),
        crate_dir.join("benches/request.php").display().to_string(),
        packs.display().to_string(),
    );
    let port = match php.eval("speedyspec://bench/request", &code) {
        Ok(HookValue::Str(s)) => serde_json::from_str::<serde_json::Value>(&s).expect("port JSON"),
        other => panic!("port run: {other:?}"),
    };

    println!("| case | stock request | port request | speed-up | stock ns/call | port ns/call |");
    println!("|---|---|---|---|---|---|");
    let mut slower = Vec::new();
    for format in ["php", "mo"] {
        for case in ["plain", "filter", "object"] {
            let key = format!("{format}/{case}");
            let (s, p) = (&stock[&key], &port[&key]);
            let (sr, pr) = (
                s["request_us"].as_f64().unwrap(),
                p["request_us"].as_f64().unwrap(),
            );
            println!(
                "| {key} | {sr:.0} us | {pr:.0} us | {:.1}x | {:.0} | {:.0} |",
                sr / pr,
                s["ns_per_call"].as_f64().unwrap(),
                p["ns_per_call"].as_f64().unwrap()
            );
            assert_eq!(
                s["check"], p["check"],
                "{key}: the port returned different strings"
            );
            if pr > sr {
                slower.push(key);
            }
        }
    }
    println!(
        "JIT on: stock {} port {}",
        stock["opcache"], port["opcache"]
    );
    let speedup = stock["php/plain"]["request_us"].as_f64().unwrap()
        / port["php/plain"]["request_us"].as_f64().unwrap();
    println!("bar: plain .l10n.php speed-up {speedup:.1}x (needs 3x); slower cases: {slower:?}");
}
