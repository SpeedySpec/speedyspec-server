//! WordPress's cron contracts (`wp-includes/cron.php`): the cron array as a plain value, the ports the scheduling
//! core needs, and its outcomes.
//!
//! - [`CronArray`], [`ScheduledEvent`], [`EventData`] and [`EventKey`] are plain values. A [`CronArray`] keeps every
//!   entry it cannot read as an event, in place, so a read-modify-write never drops what WordPress would keep.
//! - [`CronStore`] is the Repository of the `cron` option, with a compare-and-swap save (Optimistic Offline Lock).
//! - [`ArgsKeyer`], [`Clock`], [`Translator`] and [`PhpObjects`] are ports for what WordPress asks of PHP.

mod array;
mod natsort;
mod outcome;
mod ports;

pub use array::{CronArray, EventData, EventKey, ScheduledEvent, key_for};
pub use natsort::strnatcasecmp;
pub use outcome::{CronError, CronResult, RunSummary, SaveOutcome, Version};
pub use ports::{
    ArgsKeyer, Clock, CronStep, CronStore, DoingCron, ErrorReporter, NetworkSites, PhpObjects,
    SiteRef, SiteSwitcher, Translator, WakeReporter,
};

/// A PHP-shaped value, as the Hook API carries it.
pub type PhpValue = speedyspec_hooks::Value;
