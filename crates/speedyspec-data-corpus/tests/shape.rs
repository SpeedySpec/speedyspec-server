use std::collections::BTreeMap;
use std::io::Cursor;

use speedyspec_data::{DbError, DialectKind, Statement};
use speedyspec_data_corpus::{CorpusEntry, UnsupportedEntry, check, ingest, shape_of};
use speedyspec_data_query::parse_one;

#[test]
fn shape_replaces_literals() {
    let a = shape_of("SELECT * FROM wp_posts WHERE ID = 5 AND post_type IN ('a', 'b')").unwrap();
    assert_eq!(
        a,
        "SELECT * FROM wp_posts WHERE ID = ? AND post_type IN (?)"
    );
    assert_eq!(
        shape_of("SELECT * FROM wp_posts WHERE ID = 7 AND post_type IN ('c')").unwrap(),
        a
    );
}

#[test]
fn ingest_dedupes_and_counts() {
    let lines = concat!(
        r#"{"sql": "SELECT 1 FROM t WHERE a = 1", "source": "x"}"#,
        "\n",
        r#"{"sql": "SELECT 1 FROM t WHERE a = 2", "source": "y"}"#,
        "\n",
        r#"{"sql": "DELETE FROM t", "source": "x"}"#,
        "\n"
    );
    let mut corpus = BTreeMap::new();
    ingest(Cursor::new(lines), &mut corpus);
    assert_eq!(corpus.len(), 2);
    let select = corpus
        .values()
        .find(|e| e.example.starts_with("SELECT"))
        .unwrap();
    assert_eq!(select.count, 2);
    assert_eq!(select.example, "SELECT 1 FROM t WHERE a = 1");
    assert_eq!(
        select.sources.iter().cloned().collect::<Vec<_>>(),
        ["x", "y"]
    );
    assert_eq!(
        corpus
            .values()
            .find(|e| e.example.starts_with("DELETE"))
            .unwrap()
            .count,
        1
    );
}

#[test]
fn unparseable_kept_verbatim() {
    let mut corpus = BTreeMap::new();
    ingest(
        Cursor::new(r#"{"sql": "SELEC nonsense", "source": "x"}"#),
        &mut corpus,
    );
    let entry = corpus.values().next().unwrap();
    assert_eq!(
        (entry.shape.as_deref(), entry.example.as_str()),
        (None, "SELEC nonsense")
    );
}

fn entry(sql: &str) -> CorpusEntry {
    CorpusEntry {
        shape: shape_of(sql).ok(),
        example: sql.into(),
        sources: ["x".to_string()].into(),
        count: 1,
    }
}

#[test]
fn check_classifies() {
    let refuse = |_: &Statement| -> Result<(), DbError> {
        Err(DbError::Unsupported {
            construct: "X".into(),
            dialect: DialectKind::Sqlite,
        })
    };
    let entries = [entry("SELECT 1 FROM t")];
    let listed = [UnsupportedEntry {
        construct: "X".into(),
        dialect: "sqlite".into(),
        why: "w".into(),
    }];
    let report = check(&entries, DialectKind::Sqlite, &refuse, &listed, &[]);
    assert!(report.ok(), "{report:?}");
    assert_eq!(report.listed_unsupported, 1);
    let report = check(&entries, DialectKind::Sqlite, &refuse, &[], &[]);
    assert!(!report.ok());
    assert_eq!(
        report.unlisted_unsupported,
        [("SELECT 1 FROM t".to_string(), "X".to_string())]
    );
    let other = [UnsupportedEntry {
        construct: "X".into(),
        dialect: "postgres".into(),
        why: "w".into(),
    }];
    assert!(!check(&entries, DialectKind::Sqlite, &refuse, &other, &[]).ok());

    let plan =
        |s: &Statement| -> Result<(), DbError> { parse_one(&s.tree().to_string()).map(|_| ()) };
    let gap = [entry("SELEC nonsense")];
    let report = check(&gap, DialectKind::Sqlite, &plan, &[], &[]);
    assert_eq!(report.parse_failures, ["SELEC nonsense"]);
    assert!(
        check(
            &gap,
            DialectKind::Sqlite,
            &plan,
            &[],
            &["SELEC nonsense".to_string()]
        )
        .ok()
    );
    let report = check(&[entry("SELECT 1")], DialectKind::Sqlite, &plan, &[], &[]);
    assert_eq!(report.planned, 1);
}
