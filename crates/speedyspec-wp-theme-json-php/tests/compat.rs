//! `WP_Theme_JSON` and `WP_Theme_JSON_Schema` recorded from WordPress 7.1.5 (`compat/theme-json`), replayed through
//! the native classes. Arrays are compared after `compat_canon()` (PHP's `==`); strings byte for byte.
//!
//! PHP boots once per process, so this file has its own `main()` (`harness = false`).

use std::process::ExitCode;
use std::rc::Rc;

use speedyspec_compat::{replay, wordpress_release};
use speedyspec_container::Container;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, NativeArea, Php};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::WpHooksArea;
use speedyspec_wp_theme_json_php::{ThemeJsonArea, theme_json_scenario_runner};

fn main() -> ExitCode {
    let release = match wordpress_release("7.1.5") {
        Ok(root) => root,
        Err(why) => {
            println!("FAILED: the WordPress release: {why}");
            return ExitCode::FAILURE;
        }
    };
    let container = Rc::new(Container::new());
    let hooks = WpHooksArea::new(container.clone(), Rc::new(MemoryHookStore::new()));
    let theme_json = ThemeJsonArea::new(container, hooks.api());
    let areas: Vec<Rc<dyn NativeArea>> = vec![Rc::new(hooks), Rc::new(theme_json)];
    let php = match Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    }) {
        Ok(php) => php,
        Err(err) => {
            println!("FAILED: boot: {err}");
            return ExitCode::FAILURE;
        }
    };
    let mut runner = theme_json_scenario_runner(php, &release);
    let mismatches = replay("theme-json", &mut runner);
    for mismatch in &mismatches {
        let text = mismatch.to_string();
        println!("  mismatch: {}", &text[..text.floor_char_boundary(900)]);
        if let (Some(e), Some(a)) = (&mismatch.expected, &mismatch.actual) {
            let (e, a) = (format!("{:?}", e.returned), format!("{:?}", a.returned));
            println!("    expected {}", &e[..e.floor_char_boundary(1500)]);
            println!("    actual   {}", &a[..a.floor_char_boundary(1500)]);
        }
    }
    if mismatches.is_empty() {
        println!("ok: replayed_scenarios_match_wordpress");
        ExitCode::SUCCESS
    } else {
        println!(
            "FAILED: replayed_scenarios_match_wordpress: {} mismatches",
            mismatches.len()
        );
        ExitCode::FAILURE
    }
}
