//! WordPress's meta functions, recorded on MySQL (`compat/metadata`, single site and multisite), replayed exactly
//! through the native ones.
//!
//! Needs MySQL (`SPEEDYSPEC_TEST_MYSQL_URL`); without it the test reports that it was skipped. The replay runs on a
//! database of its own (`<database>_metadata`), because compat/metadata/setup.php empties `wp_options` and the other
//! tables compat/options replays on.

mod support;

use std::cell::Cell;
use std::rc::Rc;

use speedyspec_compat::{replay_for, wordpress_release};
use speedyspec_wp_meta_php::metadata_scenario_runner;

fn main() {
    let Some(url) = support::mysql("metadata") else {
        return;
    };
    let booted = support::boot(&url);
    let release = wordpress_release(&speedyspec_compat::area_wordpress("metadata"))
        .expect("the pinned release");
    let multisite = Rc::new(Cell::new(false));
    let mut runner = metadata_scenario_runner(booted.php, &release, multisite.clone())
        .expect("the release prepares");
    let mut mismatches = Vec::new();
    for network in [false, true] {
        multisite.set(network);
        let (catalogue, fixture, breaks) =
            speedyspec_compat::load_for("metadata", network).expect("the metadata files load");
        let only = std::env::var("SPEEDYSPEC_COMPAT_ONLY").ok();
        if only.is_some() || std::env::var_os("SPEEDYSPEC_COMPAT_TRACE").is_some() {
            for scenario in catalogue
                .scenarios
                .iter()
                .filter(|s| only.as_ref().is_none_or(|o| *o == s.name))
            {
                let one = speedyspec_compat::Catalogue {
                    area: catalogue.area.clone(),
                    scenarios: vec![scenario.clone()],
                };
                eprintln!("trace: {}", scenario.name);
                mismatches.extend(speedyspec_compat::replay_with(
                    &one,
                    &fixture,
                    &breaks,
                    &mut runner,
                ));
            }
        } else {
            let found = replay_for("metadata", network, &mut runner);
            let recorded = fixture.results.len();
            println!(
                "{}: {} of {recorded} scenarios match",
                if network { "multisite" } else { "single site" },
                recorded.saturating_sub(found.len())
            );
            mismatches.extend(found);
        }
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
            "FAIL: metadata_replay_matches: {} scenarios differ",
            mismatches.len()
        );
        std::process::exit(1);
    }
    println!("ok: metadata_replay_matches");
}
