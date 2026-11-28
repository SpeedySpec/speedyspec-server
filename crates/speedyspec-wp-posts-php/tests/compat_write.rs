//! WordPress's post writing, recorded on MySQL (`compat/posts-write`, single site and multisite), replayed exactly
//! through the native writers (5a: `wp_insert_post()`, `wp_update_post()`, the status transitions with their default
//! callbacks, and `set_post_type()`), beside the native reads and meta functions.
//!
//! Every scenario replays, single site and multisite, with the PHP-only steps the Rust replay leaves out: `tax_input`,
//! kses, the revisions `post_updated` writes, `_wp_old_date` and `_wp_old_slug`, and `comment_count`. The 5b writers
//! (delete, trash, untrash, publish, future posts, sticky posts, pings, auto-drafts) are still WordPress's PHP in the
//! prepared release, so their scenarios replay them as WordPress's code over the native reads and 5a writers. A
//! scenario is filtered only once it needs a 5b writer the area declares natively ([`filtered`]).
//!
//! Needs MySQL (`SPEEDYSPEC_TEST_MYSQL_URL`); without it the test reports that it was skipped. The replay runs on a
//! database of its own (`<database>_posts_write`), because compat/posts-write/setup.php empties every table it seeds.

mod support;

use std::cell::Cell;
use std::rc::Rc;

use speedyspec_compat::replay_with;
use speedyspec_wp_posts_php::{PORTED_FUNCTIONS, posts_write_scenario_runner};

/// The 5b writers: still WordPress's PHP in the prepared release (`wp_delete_post()`, `wp_trash_post()`, …), so a
/// scenario that calls them replays them as WordPress's code over the native reads and 5a writers.
const FIVE_B: &[&str] = &[
    "wp_delete_post",
    "wp_trash_post",
    "wp_untrash_post",
    "wp_publish_post",
    "check_and_publish_future_post",
    "stick_post",
    "unstick_post",
    "add_ping",
    "wp_delete_auto_drafts",
];

/// Why a scenario cannot replay yet, from the functions its steps call: one that needs a 5b writer the native area
/// declares (in [`PORTED_FUNCTIONS`]) but whose Rust use case is not ready. `None` while every 5b writer it calls
/// still runs as WordPress's PHP, which is every one today.
fn filtered(scenario: &speedyspec_compat::Scenario) -> Option<String> {
    let needs: Vec<&str> = scenario
        .steps
        .iter()
        .map(|step| step.call.as_str())
        .filter(|call| FIVE_B.contains(call) && PORTED_FUNCTIONS.contains(call))
        .collect();
    (!needs.is_empty()).then(|| format!("5b: needs the Rust {}", needs.join(", ")))
}

fn main() {
    let Some(url) = support::mysql("posts_write") else {
        return;
    };
    let (_release, root) = support::prepared("posts-write-port");
    let (php, _container) = support::boot(&url);
    let multisite = Rc::new(Cell::new(false));
    let mut runner = posts_write_scenario_runner(php, &root, multisite.clone());
    let only = std::env::var("SPEEDYSPEC_COMPAT_ONLY").ok();
    let mut mismatches = Vec::new();
    for network in [false, true] {
        multisite.set(network);
        let (catalogue, fixture, breaks) = speedyspec_compat::load_for("posts-write", network)
            .expect("the posts-write files load");
        let recorded: Vec<_> = catalogue
            .scenarios
            .iter()
            .filter(|s| fixture.results.contains_key(&s.name))
            .filter(|s| only.as_ref().is_none_or(|o| *o == s.name))
            .cloned()
            .collect();
        let (later, scenarios): (Vec<_>, Vec<_>) =
            recorded.into_iter().partition(|s| filtered(s).is_some());
        for s in &later {
            println!("filtered: {}: {}", s.name, filtered(s).unwrap_or_default());
        }
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
            "{}: {} of {total} scenarios match ({} filtered)",
            if network { "multisite" } else { "single site" },
            total.saturating_sub(found.len()),
            later.len()
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
            "FAIL: posts_write_replay_matches: {} scenarios differ",
            mismatches.len()
        );
        std::process::exit(1);
    }
    println!("ok: posts_write_replay_matches");
}
