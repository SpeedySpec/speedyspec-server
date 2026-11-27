//! Integration tests only (see `tests/`): speedyspec-wp-meta's use cases with `WpHookApi`, the Rust object cache and
//! speedyspec-wp-meta-memory. They live here so speedyspec-wp-meta keeps to contracts and has no dev-dependency on a
//! crate that depends on it, a cycle Cargo allows but release-please's cargo-workspace plugin refuses.
