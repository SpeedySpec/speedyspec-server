//! Whether an engine's dialect plans every statement of the corpus, or refuses it in a documented way.

use serde::{Deserialize, Serialize};
use speedyspec_data::{DbError, DialectKind, Statement};
use speedyspec_data_query::parse_one;

use crate::CorpusEntry;

/// A construct an engine refuses on purpose, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnsupportedEntry {
    pub construct: String,
    pub dialect: String,
    pub why: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CheckReport {
    pub planned: usize,
    pub listed_unsupported: usize,
    /// (statement, construct) refused but not listed.
    pub unlisted_unsupported: Vec<(String, String)>,
    /// Statements that do not parse and are not known parse gaps.
    pub parse_failures: Vec<String>,
    /// (statement, error) for any other failure.
    pub errors: Vec<(String, String)>,
}

impl CheckReport {
    pub fn ok(&self) -> bool {
        self.unlisted_unsupported.is_empty()
            && self.parse_failures.is_empty()
            && self.errors.is_empty()
    }
}

/// Plans each entry's example with `plan`. A refusal passes only when `unsupported` lists its construct for
/// `dialect`; a parse failure only when its text is one of `parse_gaps`.
pub fn check(
    entries: &[CorpusEntry],
    dialect: DialectKind,
    plan: &dyn Fn(&Statement) -> Result<(), DbError>,
    unsupported: &[UnsupportedEntry],
    parse_gaps: &[String],
) -> CheckReport {
    let mut report = CheckReport::default();
    for entry in entries {
        let stmt = match parse_one(&entry.example) {
            Ok(stmt) => stmt,
            Err(_) if parse_gaps.iter().any(|g| g == &entry.example) => continue,
            Err(_) => {
                report.parse_failures.push(entry.example.clone());
                continue;
            }
        };
        match plan(&stmt) {
            Ok(()) => report.planned += 1,
            Err(DbError::Unsupported { construct, .. })
                if unsupported
                    .iter()
                    .any(|u| u.construct == construct && u.dialect == dialect.to_string()) =>
            {
                report.listed_unsupported += 1;
            }
            Err(DbError::Unsupported { construct, .. }) => {
                report
                    .unlisted_unsupported
                    .push((entry.example.clone(), construct));
            }
            Err(e) => report.errors.push((entry.example.clone(), e.to_string())),
        }
    }
    report
}
