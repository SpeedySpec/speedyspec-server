//! WordPress's cron scheduling API (`wp-includes/cron.php`) in Rust, over a [`CronStore`](speedyspec_wp_cron_contracts::CronStore).
//!
//! [`CronApi`] holds one method per function, with WordPress's checks, filters and return values. Every write is a
//! compare-and-swap on the `cron` option (Optimistic Offline Lock), retried against a fresh read on conflict.
//! [`memory::MemoryCronStore`] is the store for tests; [`RustArgsKeyer`] keys arguments that hold no PHP object.

mod api;
mod keyer;
pub mod memory;
mod php;
mod read;
mod run;
mod schedules;
mod write;

pub use api::{CronApi, CronParts};
pub use keyer::{RustArgsKeyer, md5_hex};
pub use php::{array_key, is_numeric, truthy};
