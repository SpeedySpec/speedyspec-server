//! WordPress's post writing, recorded on MySQL (`compat/posts-write`, single site and multisite), replayed exactly
//! through the native writers (`wp_insert_post()`, `wp_update_post()`, the status transitions with their default
//! callbacks, `set_post_type()`, `wp_delete_post()`, `wp_trash_post()`, `wp_untrash_post()`, `wp_publish_post()`,
//! `check_and_publish_future_post()`, the sticky functions, `add_ping()`, `wp_delete_auto_drafts()` and the
//! front-page and privacy-page resets), beside the native reads, meta and option functions.
//!
//! Every scenario replays, single site and multisite, and none is filtered: the PHP-only steps the Rust replay leaves
//! out (`tax_input`, kses, the revisions `post_updated` writes, `_wp_old_date` and `_wp_old_slug`, `comment_count`),
//! the comment functions the writers hand over to, and `wp_delete_attachment()` (`delete-attachments`, through the
//! uploads port) run as WordPress's own PHP behind their ports.
//!
//! Needs MySQL (`SPEEDYSPEC_TEST_MYSQL_URL`); without it the test reports that it was skipped. The replay runs on a
//! database of its own (`<database>_posts_write`), because compat/posts-write/setup.php empties every table it seeds.

mod support;

use std::cell::Cell;
use std::rc::Rc;

use speedyspec_compat::replay_with;
use speedyspec_wp_posts_php::{PORTED_FUNCTIONS, posts_write_scenario_runner};

/// The writers the scenarios call, every one of which must be the native one: a scenario replays WordPress's
/// recording against the Rust use cases, not against a PHP copy left in the release.
const WRITERS: &[&str] = &[
    "wp_insert_post",
    "wp_update_post",
    "wp_transition_post_status",
    "set_post_type",
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

fn main() {
    for writer in WRITERS {
        assert!(
            PORTED_FUNCTIONS.contains(writer),
            "{writer} is not a native function"
        );
    }
    let Some(url) = support::mysql("posts_write") else {
        return;
    };
    let (_release, root) = support::prepared_with("posts-write-port", true);
    let (php, _container) = support::boot_with(&url, false, true);
    let multisite = Rc::new(Cell::new(false));
    let mut runner = posts_write_scenario_runner(php, &root, multisite.clone());
    let only = std::env::var("SPEEDYSPEC_COMPAT_ONLY").ok();
    let mut mismatches = Vec::new();
    for network in [false, true] {
        multisite.set(network);
        let (catalogue, fixture, breaks) = speedyspec_compat::load_for("posts-write", network)
            .expect("the posts-write files load");
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
            total.saturating_sub(found.len()),
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
