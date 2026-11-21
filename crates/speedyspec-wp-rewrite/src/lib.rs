//! WordPress's Rewrite API in Rust.
//!
//! `WP_Rewrite`'s behaviour (its default structures, rule generation with every filter, endpoints, extra rules and
//! permastructs, storage in the `rewrite_rules` option) over the router's WordPress rule sets, and request-time
//! matching through the router. The state stays in a plain [`RewriteState`]: in PHP it lives in the `$wp_rewrite`
//! object plugins write to, and a [`RewriteStateSource`] moves it between the two.

pub mod memory;
mod ports;
mod registration;
mod service;
mod state;
mod structures;

pub use ports::{
    HomeUrl, OptionsRewriteOptions, PageUris, PostTypes, PublicQueryVars, RewriteOptions,
    RewriteStateSource, UriIndex, WpLoaded,
};
pub use registration::{EndpointQueryVar, RuleQuery};
pub use service::{RewriteDeps, RewriteError, RewriteService};
pub use state::RewriteState;
