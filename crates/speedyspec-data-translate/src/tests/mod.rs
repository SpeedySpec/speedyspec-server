//! Tests that build schemas with `testing::StaticIntrospector`.
//!
//! They live inside the crate, not under `tests/`, so they see the `testing` module through `cfg(test)`.
//! The crate then needs no dev-dependency on itself to turn its own `testing` feature on; that self-edge
//! is a cycle to release-please's cargo-workspace plugin.

#[path = "../../tests/support/mod.rs"]
mod support;

mod ddl;
mod emulate;
