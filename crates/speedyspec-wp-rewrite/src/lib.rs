//! WordPress's Rewrite API in Rust.
//!
//! `WP_Rewrite`'s behaviour (its default structures, rule generation with every filter, endpoints, extra rules and
//! permastructs, storage in the `rewrite_rules` option) over the router's WordPress rule sets, and request-time
//! matching through the router. The state stays in a plain [`RewriteState`]: in PHP it lives in the `$wp_rewrite`
//! object plugins write to, and a [`RewriteStateSource`] moves it between the two.

mod generate;
pub mod memory;
mod messages;
mod ports;
mod registration;
mod routing;
mod server_rules;
mod service;
mod state;
mod storage;
mod structures;

pub use messages::*;
pub use ports::{
    FilteredRandomContentRedirects, Multisite, OptionsRewriteOptions, PageUris, PostTypes,
    PublicQueryVars, QueryableTaxonomy, RandomContentRedirects, RandomablePostType, RewriteOptions,
    RewriteStateSource, SiteTranslations, SiteUrls, Taxonomies, UriIndex, ViewablePosts, WpLoaded,
    rules_from_value, rules_to_value,
};
pub use registration::{EndpointQueryVar, RuleQuery};
pub use routing::{WpOutcome, WpRouting};
pub use service::{RewriteDeps, RewriteError, RewriteService};
pub use state::RewriteState;
