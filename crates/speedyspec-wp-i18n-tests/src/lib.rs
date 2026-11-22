//! Integration tests only (see `tests/`): speedyspec-wp-i18n with speedyspec-wp-i18n-files and
//! speedyspec-wp-i18n-memory. They live here rather than in any of the three so that none of them has a
//! dev-dependency on a crate that depends on it, a cycle Cargo allows but release-please's cargo-workspace plugin
//! refuses.
