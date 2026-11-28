//! WordPress's post writing, recorded on MySQL (`compat/posts-write`, single site and multisite), replayed exactly
//! through the native writers (5a: `wp_insert_post()`, `wp_update_post()`, the status transitions with their default
//! callbacks, and `set_post_type()`), beside the native reads and meta functions.
//!
//! Every `insert-*`, `update-*`, `transition-*` and `set-type*` scenario replays, the multisite ones that insert and
//! update included, with the PHP-only steps the Rust replay leaves out: `tax_input`, kses, the revisions
//! `post_updated` writes, `_wp_old_date` and `_wp_old_slug`, and `comment_count`. The scenarios of the 5b writers
//! (delete, trash, untrash, publish, future posts, sticky posts, pings, auto-drafts) are filtered out until they are
//! native.
//!
//! Needs MySQL (`SPEEDYSPEC_TEST_MYSQL_URL`); without it the test reports that it was skipped. The replay runs on a
//! database of its own (`<database>_posts_write`), because compat/posts-write/setup.php empties every table it seeds.

mod support;

use std::cell::Cell;
use std::rc::Rc;

use speedyspec_compat::replay_with;
use speedyspec_wp_posts_php::posts_write_scenario_runner;

/// The scenarios of the native writers (5a).
fn native(name: &str) -> bool {
    ["insert-", "update-", "transition-", "set-type"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
        || ["ms-insert-blog-one", "ms-switch-insert"].contains(&name)
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
        let (scenarios, later): (Vec<_>, Vec<_>) =
            recorded.into_iter().partition(|s| native(&s.name));
        for s in &later {
            println!("filtered: {}: 5b", s.name);
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
            "{}: {} of {total} scenarios match ({} filtered: 5b)",
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
