//! WordPress's `wpdb`, recorded on MySQL (`compat/wp-db`), replayed through the native class on SQLite. Where SQLite
//! cannot answer as MySQL does, `breaks.json`'s `engine_breaks.sqlite` says why.

mod common;

use std::collections::BTreeMap;
use std::rc::Rc;

use speedyspec_compat::{Breaks, load, replay_with};
use speedyspec_data::EngineConfig;
use speedyspec_data_sqlite::SqliteEngineFactory;
use speedyspec_wp_db::WpDbConfig;

fn runner() -> speedyspec_compat::PhpRunner {
    // support.php reads its DB_* constants from this URL; on SQLite they are only names.
    if std::env::var_os("SPEEDYSPEC_TEST_MYSQL_URL").is_none() {
        // SAFETY: set before any other thread starts.
        unsafe {
            std::env::set_var(
                "SPEEDYSPEC_TEST_MYSQL_URL",
                "mysql://root:@localhost/speedyspec_test",
            )
        };
    }
    common::wp_db_runner(
        Rc::new(SqliteEngineFactory),
        EngineConfig {
            name: "default".into(),
            url: "sqlite://:memory:".into(),
            options: BTreeMap::from([("database".into(), "speedyspec_test".into())]),
        },
        WpDbConfig {
            user: "root".into(),
            password: String::new(),
            name: "speedyspec_test".into(),
            host: "localhost".into(),
            charset: Some("utf8mb4".into()),
            collate: Some(String::new()),
            table_prefix: "wp_".into(),
        },
    )
}

fn main() {
    let (catalogue, fixture, shared) = load("wp-db").expect("the compatibility files load");
    let mut runner = runner();
    let mismatches = replay_with(
        &catalogue,
        &fixture,
        &common::breaks_for_engine("wp-db", "sqlite"),
        &mut runner,
    );
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
        eprintln!("{} scenario(s) differ on SQLite", mismatches.len());
        std::process::exit(1);
    }
    println!("ok: wp_db_replays_on_sqlite");

    let stale_check = replay_with(
        &catalogue,
        &fixture,
        &Breaks {
            breaks: shared.breaks,
        },
        &mut runner,
    );
    let mut stale = Vec::new();
    for scenario in common::engine_break_scenarios("wp-db", "sqlite") {
        if !stale_check.iter().any(|m| m.scenario == scenario) {
            stale.push(scenario);
        }
    }
    if !stale.is_empty() {
        eprintln!("engine_breaks.sqlite entries no longer needed: {stale:?}");
        std::process::exit(1);
    }
    println!("ok: sqlite_breaks_are_still_needed");
}
