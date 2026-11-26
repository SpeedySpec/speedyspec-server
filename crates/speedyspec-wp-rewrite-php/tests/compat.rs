//! WordPress's rewrite API, recorded from the pinned release (`compat/rewrite`), replayed exactly through the port.
//! PHP boots once per process (`harness = false`). No database is needed: options are stubbed the same on both sides.

use std::rc::Rc;

use speedyspec_compat::{replay, wordpress_release};
use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, NativeArea, Php};
use speedyspec_php_ext::twins_area;
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::WpHooksArea;
use speedyspec_wp_rewrite_php::{PhpRewriteOptions, RewriteArea, rewrite_scenario_runner};

fn main() {
    let container = Rc::new(Container::new());
    let hooks = WpHooksArea::new(container.clone(), Rc::new(MemoryHookStore::new()));
    let rewrite = RewriteArea::new(container.clone(), hooks.api(), Rc::new(PhpRewriteOptions));
    let areas: Vec<Rc<dyn NativeArea>> = vec![
        twins_area(),
        Rc::new(ContainerArea::new("compat", container)),
        Rc::new(hooks),
        Rc::new(rewrite),
    ];
    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    })
    .expect("PHP boots");
    let release = wordpress_release(&speedyspec_compat::area_wordpress("rewrite"))
        .expect("the pinned release");
    let mut runner = rewrite_scenario_runner(php, &release);
    let mismatches = replay("rewrite", &mut runner);
    for m in &mismatches {
        eprintln!("MISMATCH {m}");
        let show = |r: &Option<speedyspec_compat::StepResult>| {
            r.as_ref()
                .map(|r| serde_json::to_string(r).unwrap_or_default())
                .unwrap_or_default()
        };
        eprintln!("  expected: {}", show(&m.expected));
        eprintln!("  actual:   {}", show(&m.actual));
    }
    if mismatches.is_empty() {
        println!("ok: rewrite_replay_matches");
    } else {
        eprintln!(
            "FAIL: rewrite_replay_matches: {} scenarios differ",
            mismatches.len()
        );
        std::process::exit(1);
    }
}
