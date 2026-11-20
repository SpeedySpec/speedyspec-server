//! Every statement of the SQL corpus translates for PostgreSQL, and PostgreSQL accepts what it translates to.

mod common;

use std::collections::BTreeMap;
use std::rc::Rc;

use common::pg_url;
use speedyspec_data::{
    DbError, DialectKind, EngineConfig, Introspector, Prepared, SqlStore, StatementKind,
};
use speedyspec_data_corpus::{CorpusEntry, check, load_corpus, load_parse_gaps, load_unsupported};
use speedyspec_data_postgres::{PostgresDialect, PostgresStore};
use speedyspec_data_query::parse_one;
use speedyspec_data_translate::testing::StaticIntrospector;
use sqlparser::ast::Statement as Ast;

/// The corpus's CREATE TABLEs, WordPress's schema first; a table made twice keeps its first form.
fn creates(corpus: &[CorpusEntry]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    let schema = corpus
        .iter()
        .filter(|e| e.sources.contains("wordpress-schema"));
    let rest = corpus
        .iter()
        .filter(|e| !e.sources.contains("wordpress-schema"));
    for entry in schema.chain(rest) {
        if let Ok(stmt) = parse_one(&entry.example)
            && let Ast::CreateTable(c) = stmt.tree()
        {
            let name = speedyspec_data_translate::object_name(&c.name);
            if !names.contains(&name)
                && speedyspec_data_translate::catalogue_table(stmt.tree()).is_ok()
            {
                names.push(name);
                out.push(entry.example.clone());
            }
        }
    }
    out
}

#[test]
fn corpus_translates_for_postgres() {
    let corpus = load_corpus();
    let ddl = creates(&corpus);
    let refs: Vec<&str> = ddl.iter().map(String::as_str).collect();
    let introspector: Rc<dyn Introspector> = Rc::new(StaticIntrospector::from_mysql_ddl(&refs));
    let dialect = PostgresDialect::new(Some(introspector));
    let gaps: Vec<String> = load_parse_gaps().into_iter().map(|g| g.sql).collect();
    let report = check(
        &corpus,
        DialectKind::Postgres,
        &|s| dialect.plan(s).map(|_| ()),
        &load_unsupported(),
        &gaps,
    );
    println!(
        "planned {}, listed refusals {}, unlisted refusals {}, parse failures {}, errors {}",
        report.planned,
        report.listed_unsupported,
        report.unlisted_unsupported.len(),
        report.parse_failures.len(),
        report.errors.len()
    );
    for (sql, construct) in &report.unlisted_unsupported {
        println!(
            "UNSUPPORTED {construct}: {}",
            sql.chars().take(300).collect::<String>()
        );
    }
    for (sql, error) in &report.errors {
        println!(
            "ERROR {error}: {}",
            sql.chars().take(300).collect::<String>()
        );
    }
    assert!(report.ok());
}

#[test]
#[ignore = "needs PostgreSQL: set SPEEDYSPEC_TEST_POSTGRES_URL"]
fn corpus_prepares_on_postgres() {
    let schema = "t_corpus";
    let mut admin = postgres::Client::connect(&pg_url(), postgres::NoTls).unwrap();
    // One table at a time: a schema of thousands of objects dropped at once exceeds the lock table.
    let tables: Vec<String> = admin
        .query(
            "SELECT tablename FROM pg_tables WHERE schemaname = $1",
            &[&schema],
        )
        .unwrap()
        .iter()
        .map(|r| r.get(0))
        .collect();
    for table in tables {
        admin
            .batch_execute(&format!(
                "DROP TABLE IF EXISTS {schema}.\"{table}\" CASCADE"
            ))
            .unwrap();
    }
    admin
        .batch_execute(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
        .unwrap();
    let store = PostgresStore::open(&EngineConfig {
        name: "corpus".into(),
        url: pg_url(),
        options: BTreeMap::from([("schema".to_string(), schema.to_string())]),
    })
    .unwrap();
    let corpus = load_corpus();
    // WordPress's tables under the test suite's prefix too, which most of the corpus uses.
    let core: Vec<String> = corpus
        .iter()
        .filter(|e| e.sources.contains("wordpress-schema"))
        .map(|e| {
            e.example
                .replacen("CREATE TABLE wp_", "CREATE TABLE wptests_", 1)
        })
        .collect();
    for sql in creates(&corpus).into_iter().chain(core) {
        let _ = store.execute(&parse_one(&sql).unwrap());
    }
    // Multisite's columns on the users table, which its install adds.
    let _ = store.execute(&parse_one("ALTER TABLE wptests_users ADD COLUMN spam tinyint(2) NOT NULL DEFAULT 0, ADD COLUMN deleted tinyint(2) NOT NULL DEFAULT 0").unwrap());
    admin
        .batch_execute(&format!("SET search_path = {schema}"))
        .unwrap();
    let (mut prepared, mut unverified, mut failures) = (0, 0, Vec::new());
    for entry in &corpus {
        let Ok(stmt) = parse_one(&entry.example) else {
            continue;
        };
        if matches!(
            stmt.kind(),
            StatementKind::Ddl | StatementKind::Transaction(_) | StatementKind::Set
        ) {
            continue;
        }
        let Ok(Prepared::Sql(emitted)) = store.dialect().prepare(&stmt) else {
            continue;
        };
        for e in emitted {
            match admin.prepare(&e.sql) {
                Ok(_) => prepared += 1,
                Err(err) => match err.code().map(|c| c.code()) {
                    Some("42P01") => unverified += 1,
                    _ => failures.push(format!(
                        "{}\n    => {}\n    from {}",
                        e.sql,
                        err.as_db_error().map_or_else(
                            || err.to_string(),
                            |d| format!("{}: {}", d.code().code(), d.message())
                        ),
                        entry.example.chars().take(200).collect::<String>()
                    )),
                },
            }
        }
    }
    println!(
        "prepared {prepared}, unverified (plugin tables) {unverified}, failed {}",
        failures.len()
    );
    for f in failures.iter().take(80) {
        println!("FAIL {f}");
    }
    assert!(failures.is_empty());
    let _ = DbError::Config(String::new());
}
