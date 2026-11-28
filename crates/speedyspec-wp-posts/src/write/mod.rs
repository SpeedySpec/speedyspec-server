//! Writing posts: `wp_insert_post()`, `wp_update_post()`, `wp_transition_post_status()` and its default callbacks,
//! and `set_post_type()`, behind the write contracts, with their messages and the outbound ports they call.
//!
//! The rest of `post.php`'s writers (delete, trash, untrash, publish, sticky posts, pings and auto-drafts) come
//! with phase 5b, which also implements the [`PostWrites`](speedyspec_wp_posts_contracts::PostWrites) contract over
//! all of them; until then these are [`PostWritesApi`]'s methods and messages.

mod api;
mod insert;
mod messages;
mod php;
mod ports;
mod transition;
mod update;

pub use api::{PostWritesApi, WriteDeps};
pub use messages::*;
pub use ports::{
    PlainPostObjects, PostObjects, Runtime, Scheduling, SlugSettings, WriteFormatting,
};
pub use transition::{
    future_post_hook_callback, register_future_post_hook, register_post_write_callbacks,
};
