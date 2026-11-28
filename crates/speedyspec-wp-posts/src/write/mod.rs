//! Writing posts: `post.php`'s writers behind the [`PostWrites`](speedyspec_wp_posts_contracts::PostWrites) contract
//! ([`PostWritesApi`]), with their messages, their default callbacks and the outbound ports they call.
//!
//! - `wp_insert_post()` ([`insert`]), `wp_update_post()` and `set_post_type()` ([`update`]);
//! - `wp_transition_post_status()` and its default callbacks ([`transition`]);
//! - `wp_delete_post()` and the front-page and privacy-page resets ([`delete`]);
//! - `wp_trash_post()` and `wp_untrash_post()` ([`trash`]);
//! - `wp_publish_post()` and `check_and_publish_future_post()` ([`publish`]);
//! - `stick_post()` and `unstick_post()` ([`sticky`]), `add_ping()` ([`pings`]) and `wp_delete_auto_drafts()`
//!   ([`drafts`]).

mod api;
mod contract;
mod delete;
mod drafts;
mod error;
mod insert;
mod messages;
mod php;
mod pings;
mod ports;
mod publish;
mod sticky;
mod transition;
mod trash;
mod update;

pub use api::{PostWritesApi, WriteDeps};
pub use error::{PhpThrow, php_throw};
pub use messages::*;
pub use ports::{
    PlainPostObjects, PostObjects, Runtime, Scheduling, SlugSettings, WriteFormatting,
};
pub use transition::{
    future_post_hook_callback, register_future_post_hook, register_post_write_callbacks,
};
