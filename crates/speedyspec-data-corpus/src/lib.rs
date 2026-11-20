//! The SQL corpus: the statements WordPress and popular plugins send, grouped by shape, and the check that an
//! engine's dialect plans every one of them (or refuses it in a documented way).

mod check;
mod shape;

pub use check::{CheckReport, UnsupportedEntry, check};
pub use shape::{CorpusEntry, ingest, shape_of};

use std::path::PathBuf;

/// `compat/sql-corpus`, where the corpus and its lists are kept.
pub fn corpus_dir() -> PathBuf {
    speedyspec_compat::compat_dir().join("sql-corpus")
}

/// A statement text `sqlparser` rejects, with the upstream issue that tracks it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ParseGap {
    pub sql: String,
    pub issue: String,
    /// Written by one of WordPress's tests to exercise `wpdb` (invalid SQL, a stored procedure), not sent by
    /// WordPress itself.
    #[serde(default)]
    pub from_test: bool,
}

fn read<T: serde::de::DeserializeOwned + Default>(name: &str) -> T {
    let path = corpus_dir().join(name);
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        }
        Err(_) => T::default(),
    }
}

/// The committed corpus.
pub fn load_corpus() -> Vec<CorpusEntry> {
    read("corpus.json")
}

/// The committed refusals.
pub fn load_unsupported() -> Vec<UnsupportedEntry> {
    read("unsupported.json")
}

/// The committed parse gaps.
pub fn load_parse_gaps() -> Vec<ParseGap> {
    read("parse-gaps.json")
}
