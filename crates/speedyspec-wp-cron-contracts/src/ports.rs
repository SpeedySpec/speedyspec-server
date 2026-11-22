//! The ports the scheduling core needs.

use speedyspec_hooks::Value;

use crate::{CronError, EventKey, SaveOutcome, Version};

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
