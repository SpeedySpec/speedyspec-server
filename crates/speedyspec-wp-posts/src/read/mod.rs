//! Reading posts: the `post.php` read use cases behind the [`Posts`](speedyspec_wp_posts_contracts::Posts) contract,
//! the `posts` cache functions, their messages, and the outbound ports they call.

mod api;
mod cache;
mod counts;
mod entry;
mod get;
mod messages;
mod pages;
mod pings;
mod ports;

pub use api::{PostRead, PostsApi, PostsDeps, read_hooks};
pub use entry::PostEntry;
pub use get::PostObject;
pub use messages::*;
pub use ports::{CacheInvalidation, Microtime, Options, Taxonomy, Users};
