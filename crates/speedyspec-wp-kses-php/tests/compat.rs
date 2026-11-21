//! WordPress's kses functions, recorded from the 7.1.5 release (`compat/kses`), replayed exactly through the native
//! ones: every step's result, every recorded hook invocation and the hook globals (counts included).

use std::rc::Rc;

use speedyspec_compat::{replay, wordpress_release};
use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, NativeArea, Php};
use speedyspec_php_ext::twins_area;
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::WpHooksArea;
use speedyspec_wp_kses_php::{KsesArea, kses_scenario_runner};

fn main() {
    let container = Rc::new(Container::new());
    let hooks = WpHooksArea::new(container.clone(), Rc::new(MemoryHookStore::new()));
    let kses = KsesArea::new(container.clone(), hooks.api());
    let areas: Vec<Rc<dyn NativeArea>> = vec![
        twins_area(),
        Rc::new(ContainerArea::new("compat", container)),
        Rc::new(hooks),
        Rc::new(kses),
    ];
    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    })
    .expect("PHP boots");
    let release = wordpress_release("7.1.5").expect("the pinned release");
    let mut runner = kses_scenario_runner(php, &release).expect("the release prepares");
    // SPEEDYSPEC_COMPAT_TRACE=1 replays one scenario at a time, naming each and its time; SPEEDYSPEC_COMPAT_ONLY
    // picks one.
    let mut mismatches = Vec::new();
    if std::env::var_os("SPEEDYSPEC_COMPAT_TRACE").is_some() {
        let (catalogue, fixture, breaks) =
            speedyspec_compat::load("kses").expect("the kses files load");
        let only = std::env::var("SPEEDYSPEC_COMPAT_ONLY").ok();
        for scenario in catalogue
            .scenarios
            .iter()
            .filter(|s| only.as_ref().is_none_or(|o| *o == s.name))
        {
            let one = speedyspec_compat::Catalogue {
                area: catalogue.area.clone(),
                scenarios: vec![scenario.clone()],
            };
            let started = std::time::Instant::now();
            eprintln!("trace: {} ...", scenario.name);
            mismatches.extend(speedyspec_compat::replay_with(
                &one,
                &fixture,
                &breaks,
                &mut runner,
            ));
            eprintln!("trace: {} {:?}", scenario.name, started.elapsed());
        }
    } else {
        mismatches = replay("kses", &mut runner);
    }
    for m in &mismatches {
        eprintln!("MISMATCH {m}");
        let show = |r: &Option<speedyspec_compat::StepResult>| {
            r.as_ref()
                .map(|e| serde_json::to_string(e).unwrap_or_default())
                .unwrap_or_default()
        };
        eprintln!("  expected: {}", show(&m.expected));
        eprintln!("  actual:   {}", show(&m.actual));
    }
    if !mismatches.is_empty() {
        eprintln!(
            "FAIL: kses_replay_matches: {} scenarios differ",
            mismatches.len()
        );
        std::process::exit(1);
    }
    println!("ok: kses_replay_matches");
}
