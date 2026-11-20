//! Every statement of the SQL corpus plans for SQLite, or is a documented refusal or parse gap.

use std::collections::BTreeMap;

use speedyspec_data::{DialectKind, EngineConfig, SqlStore};
use speedyspec_data_corpus::{check, load_corpus, load_parse_gaps, load_unsupported};
use speedyspec_data_query::parse_one;
use speedyspec_data_sqlite::SqliteStore;

#[test]
fn corpus_translates_for_sqlite() {
    let store = SqliteStore::open(&EngineConfig {
        name: "corpus".into(),
        url: "sqlite://:memory:".into(),
        options: BTreeMap::new(),
    })
    .unwrap();
    let corpus = load_corpus();
    // WordPress's own tables, so statements that read the schema find it.
    for entry in corpus
        .iter()
        .filter(|e| e.sources.contains("wordpress-schema"))
    {
        let stmt = parse_one(&entry.example).unwrap();
        store
            .execute(&stmt)
            .unwrap_or_else(|e| panic!("{}: {e}", entry.example));
    }
    // The tables tests and plugins make for themselves (temporary ones too), so an ALTER TABLE finds the table it
    // changes; a table made twice in the corpus keeps its first form.
    for entry in &corpus {
        if let Ok(stmt) = parse_one(&entry.example)
            && matches!(stmt.tree(), sqlparser::ast::Statement::CreateTable(_))
        {
            let _ = store.execute(&stmt);
        }
    }
    let gaps: Vec<String> = load_parse_gaps().into_iter().map(|g| g.sql).collect();
    let report = check(
        &corpus,
        DialectKind::Sqlite,
        &|s| store.plan(s).map(|_| ()),
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
        println!("UNSUPPORTED {construct}: {sql}");
    }
    for sql in &report.parse_failures {
        println!("PARSE {sql}");
    }
    for (sql, error) in &report.errors {
        println!("ERROR {error}: {sql}");
    }
    assert!(report.ok());
}
