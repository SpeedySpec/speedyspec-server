//! WordPress's post reading and post cache functions, recorded on MySQL (`compat/posts-read`, single site and
//! multisite), replayed exactly through the native ones, beside the native meta functions.
//!
//! Needs MySQL (`SPEEDYSPEC_TEST_MYSQL_URL`); without it the test reports that it was skipped. The replay runs on a
//! database of its own (`<database>_posts_read`), because compat/posts-read/setup.php empties every table it seeds.

mod support;

use std::cell::Cell;
use std::rc::Rc;

use speedyspec_compat::replay_with;
use speedyspec_wp_posts_php::posts_read_scenario_runner;

fn main() {
    let Some(url) = support::mysql("posts_read") else {
        return;
    };
    let (_release, root) = support::prepared("posts-read-port");
    let (php, _container) = support::boot(&url);
    let multisite = Rc::new(Cell::new(false));
    let mut runner = posts_read_scenario_runner(php, &root, multisite.clone());
    let only = std::env::var("SPEEDYSPEC_COMPAT_ONLY").ok();
    let mut mismatches = Vec::new();
    for network in [false, true] {
        multisite.set(network);
        let (catalogue, fixture, breaks) =
            speedyspec_compat::load_for("posts-read", network).expect("the posts-read files load");
        let scenarios: Vec<_> = catalogue
            .scenarios
            .iter()
            .filter(|s| fixture.results.contains_key(&s.name))
            .filter(|s| only.as_ref().is_none_or(|o| *o == s.name))
            .cloned()
            .collect();
        let total = scenarios.len();
        let trace = std::env::var_os("SPEEDYSPEC_COMPAT_TRACE").is_some();
        // Traced, each scenario runs alone with its name printed first, so a hang names its scenario.
        let batches: Vec<Vec<_>> = if trace {
            scenarios.into_iter().map(|s| vec![s]).collect()
        } else {
            vec![scenarios]
        };
        let mut found = Vec::new();
        for scenarios in batches {
            if trace {
                eprintln!("trace: {}", scenarios[0].name);
            }
            found.extend(replay_with(
                &speedyspec_compat::Catalogue {
                    area: catalogue.area.clone(),
                    scenarios,
                },
                &fixture,
                &breaks,
                &mut runner,
            ));
        }
        println!(
            "{}: {} of {total} scenarios match",
            if network { "multisite" } else { "single site" },
            total.saturating_sub(found.len())
        );
        mismatches.extend(found);
    }
    for m in &mismatches {
        eprintln!("MISMATCH {m}");
        let json = |v: &Option<speedyspec_compat::StepResult>| {
            v.as_ref()
                .map(|e| serde_json::to_string(e).unwrap_or_default())
                .unwrap_or_default()
        };
        eprintln!("  expected: {}", json(&m.expected));
        eprintln!("  actual:   {}", json(&m.actual));
    }
    if !mismatches.is_empty() {
        eprintln!(
            "FAIL: posts_read_replay_matches: {} scenarios differ",
            mismatches.len()
        );
        std::process::exit(1);
    }
    println!("ok: posts_read_replay_matches");
}
