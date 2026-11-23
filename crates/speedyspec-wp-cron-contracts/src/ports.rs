//! The ports the scheduling core needs.

use speedyspec_hooks::Value;

use crate::{CronError, CronResult, EventKey, SaveOutcome, Version};

/// The `cron` option (Repository). Values are the option as `get_option( 'cron' )` returns it, `version` included.
pub trait CronStore {
    /// `get_option( 'cron' )`, its hooks firing, and the version it would store as.
    fn load(&self) -> Result<(Value, Version), CronError>;

    /// Forgets this process's cache of the option and loads again. The `bool` is true when the value read is the
    /// stored one (no filter rewrote it), so a compare-and-swap against it can succeed.
    fn reload(&self) -> Result<(Value, Version, bool), CronError>;

    /// `update_option( 'cron', $value, true )` where the stored bytes are still `expected`.
    fn save_if(&self, expected: &Version, value: Value) -> Result<SaveOutcome, CronError>;

    /// `update_option( 'cron', $value, true )`.
    fn save(&self, value: Value) -> Result<bool, CronError>;
}

/// `md5( serialize( $args ) )`.
pub trait ArgsKeyer {
    fn key(&self, args: &Value) -> Result<EventKey, CronError>;
}

/// `microtime( true )` and `time()`, and a pause between compare-and-swap attempts.
pub trait Clock {
    fn microtime(&self) -> f64;
    fn time(&self) -> i64;
    /// Waits about `millis` milliseconds (a test clock need not wait).
    fn pause(&self, millis: u64);
}

/// `__( $text )`.
pub trait Translator {
    fn translate(&self, text: &str) -> String;
}

/// What only PHP can do: the `stdClass` events WordPress hands to filters, reading an object's property, and
/// recognising a `WP_Error`.
pub trait PhpObjects {
    /// `(object) $array`: `wp_get_scheduled_event()` builds its event this way.
    fn to_object(&self, array: Value) -> Value;
    /// `$object->$name`, or `None` when it is unset or `object` is not an object.
    fn property(&self, object: &Value, name: &str) -> Option<Value>;
    fn is_wp_error(&self, value: &Value) -> bool;
}

/// Where the next due time goes: the runtime's report to the server. The earliest time reported in a request wins.
pub trait WakeReporter {
    fn wake_at(&self, timestamp: i64);
}

/// The `doing_cron` transient, which the runner keeps for display (it is not the lock; the claim is).
pub trait DoingCron {
    fn get(&self) -> Result<Option<String>, CronError>;
    fn set(&self, value: &str) -> Result<(), CronError>;
    fn delete(&self) -> Result<(), CronError>;
}

/// The step of a due event that failed: rescheduling it, or unscheduling it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CronStep {
    Reschedule,
    Unschedule,
}

/// `wp-cron.php`'s error reporting for a failed step.
pub trait ErrorReporter {
    /// Logs the failure as `wp-cron.php` does (`error_log()` of the translated message with the hook, the error's
    /// code and message, and the event as JSON) and returns the `WP_Error` to hand to the step's error action.
    fn report(&self, step: CronStep, hook: &Value, error: &CronResult, event: &Value) -> Value;
}

/// A site of the network, as the server's follow-up call addresses it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SiteRef {
    /// The site's id (`blog_id`).
    pub id: u64,
    /// The site's domain, the `Host` its request is booted with.
    pub domain: String,
    /// The site's path, the request URI its request is booted with.
    pub path: String,
}

/// The network's sites. A single-site install answers `is_multisite()` false and is asked nothing else.
pub trait NetworkSites {
    fn is_multisite(&self) -> bool;
    /// The current network's active sites (not spam, deleted or archived), in id order.
    fn sites(&self) -> Result<Vec<SiteRef>, CronError>;
    /// The site this request is booted as.
    fn current(&self) -> u64;
}

/// `switch_to_blog()` and `restore_current_blog()`: the database tables, object cache and options follow.
pub trait SiteSwitcher {
    fn switch_to(&self, site: u64) -> Result<(), CronError>;
    fn restore(&self) -> Result<(), CronError>;
}
