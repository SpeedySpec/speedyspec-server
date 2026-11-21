//! The committed corpus: every statement parses, or is a known parse gap that no core-only statement hits.

use speedyspec_data_corpus::{load_corpus, load_parse_gaps};
use speedyspec_data_query::parse_one;

const CORE: &[&str] = &["wordpress-phpunit", "wordpress-schema"];

#[test]
fn corpus_parses() {
    let corpus = load_corpus();
    assert!(!corpus.is_empty(), "compat/sql-corpus/corpus.json is empty");
    let gaps = load_parse_gaps();
    let mut failures = Vec::new();
    for entry in &corpus {
        if parse_one(&entry.example).is_err() && !gaps.iter().any(|g| g.sql == entry.example) {
            failures.push(entry.example.clone());
        }
    }
    assert!(
        failures.is_empty(),
        "{} statements do not parse:\n{}",
        failures.len(),
        failures.join("\n")
    );
    for gap in &gaps {
        assert!(!gap.issue.is_empty(), "{} has no upstream issue", gap.sql);
        let entry = corpus.iter().find(|e| e.example == gap.sql);
        if let Some(entry) = entry {
            let core_only = entry.sources.iter().all(|s| CORE.contains(&s.as_str()));
            assert!(
                !core_only || gap.from_test,
                "a parse gap from core alone: {}",
                gap.sql
            );
        }
    }
}
