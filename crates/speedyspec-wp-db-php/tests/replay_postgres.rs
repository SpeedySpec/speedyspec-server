//! WordPress's `wpdb`, recorded on MySQL (`compat/wp-db`), replayed through the native class on PostgreSQL. Where PostgreSQL
//! cannot answer as MySQL does, `breaks.json`'s `engine_breaks.postgres` says why.

mod common;

use std::collections::BTreeMap;
use std::rc::Rc;

use speedyspec_compat::{Breaks, load, replay_with};
use speedyspec_data::EngineConfig;
use speedyspec_data_postgres::PostgresEngineFactory;
use speedyspec_wp_db::WpDbConfig;

fn runner(url: String) -> speedyspec_compat::PhpRunner {
    // support.php reads its DB_* constants from this URL; on PostgreSQL they are only names.
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
        Rc::new(PostgresEngineFactory),
        EngineConfig {
            name: "default".into(),
            url: url.clone(),
            options: BTreeMap::from([("schema".into(), "t_wp_db_replay".into())]),
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
    let Ok(url) = std::env::var("SPEEDYSPEC_TEST_POSTGRES_URL") else {
        println!("skipped: needs PostgreSQL: set SPEEDYSPEC_TEST_POSTGRES_URL");
        return;
    };
    let mut admin = postgres::Client::connect(&url, postgres::NoTls).expect("PostgreSQL is up");
    admin
        .batch_execute("DROP SCHEMA IF EXISTS t_wp_db_replay CASCADE")
        .unwrap();
    let (catalogue, fixture, shared) = load("wp-db").expect("the compatibility files load");
    let mut runner = runner(url);
    let mismatches = replay_with(
        &catalogue,
        &fixture,
        &common::breaks_for_engine("wp-db", "postgres"),
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
        eprintln!("{} scenario(s) differ on PostgreSQL", mismatches.len());
        std::process::exit(1);
    }
    println!("ok: wp_db_replays_on_postgres");

    let stale_check = replay_with(
        &catalogue,
        &fixture,
        &Breaks {
            breaks: shared.breaks,
        },
        &mut runner,
    );
    let mut stale = Vec::new();
    for scenario in common::engine_break_scenarios("wp-db", "postgres") {
        if !stale_check.iter().any(|m| m.scenario == scenario) {
            stale.push(scenario);
        }
    }
    if !stale.is_empty() {
        eprintln!("engine_breaks.postgres entries no longer needed: {stale:?}");
        std::process::exit(1);
    }
    println!("ok: postgres_breaks_are_still_needed");
}
