//! Integration tests only (see `tests/`): speedyspec-wp-options with speedyspec-wp-options-memory. They live
//! here rather than in speedyspec-wp-options so that it has no dev-dependency on a crate that depends on it, a cycle
//! Cargo allows but release-please's cargo-workspace plugin refuses.
