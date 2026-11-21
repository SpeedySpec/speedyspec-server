//! The router.
//!
//! A Laravel-shaped registration API (Builder, with `Router` as the Facade over collection, compilation, dispatch
//! and URL generation) over matching that takes FastRoute's ideas: static paths by one hash lookup, dynamic routes
//! combined into a few large regexes, compiled data built once and cached by fingerprint. WordPress's routing
//! concepts are part of the core: ordered rule sets with query templates, guards and decode retry, and permastruct
//! expansion.

pub mod dispatch;
mod group;
pub mod pattern;
mod router;
mod url;

pub use group::Group;
pub use router::{CompileError, RouteBuilder, Router, RuleSlot, Target, fingerprint_of};
pub use url::UrlError;
mod cache;
mod table;

pub use cache::TableCache;
pub use table::{CompiledTable, RouteRequest, WpSubject};
