//! The cron ports' PHP adapters.

use speedyspec_hooks::Key;
use speedyspec_wp_cron::{RustArgsKeyer, md5_hex};
use speedyspec_wp_cron_contracts::{
    ArgsKeyer, Clock, CronError, CronResult, CronStep, DoingCron, ErrorReporter, EventKey,
    NetworkSites, PhpObjects, SiteSwitcher, Translator,
};
use speedyspec_wp_hooks::HookValue;

fn call(function: &str, args: &[HookValue]) -> Option<HookValue> {
    speedyspec_php::call(&HookValue::Str(function.into()), args).ok()
}

fn function_exists(name: &str) -> bool {
    matches!(
        call("function_exists", &[HookValue::Str(name.into())]),
        Some(HookValue::Bool(true))
    )
}

/// `md5( serialize( $args ) )`: in Rust when the arguments hold no PHP object, through PHP's `serialize()` when they
/// do (Rust keeps PHP objects opaque).
pub struct PhpArgsKeyer;

impl ArgsKeyer for PhpArgsKeyer {
    fn key(&self, args: &HookValue) -> Result<EventKey, CronError> {
        if let Ok(key) = RustArgsKeyer.key(args) {
            return Ok(key);
        }
        let serialized = speedyspec_php::call_catching(
            &HookValue::Str("serialize".into()),
            std::slice::from_ref(args),
        )
        .map_err(|failure| CronError {
            message: failure.error.to_string(),
            payload: failure
                .exception
                .map(|e| std::rc::Rc::new(e) as std::rc::Rc<dyn std::any::Any>),
        })?;
        let bytes = match serialized {
            HookValue::Str(s) => s.into_bytes(),
            HookValue::Bytes(b) => b,
            other => return Err(CronError::new(format!("serialize() returned {other:?}"))),
        };
        Ok(EventKey::found(md5_hex(&bytes)))
    }
}

/// WordPress's `__()` when it is loaded; the text itself otherwise.
pub struct PhpTranslator;

impl Translator for PhpTranslator {
    fn translate(&self, text: &str) -> String {
        if !function_exists("__") {
            return text.to_string();
        }
        match call("__", &[HookValue::Str(text.into())]) {
            Some(HookValue::Str(s)) => s,
            Some(HookValue::Bytes(b)) => String::from_utf8_lossy(&b).into_owned(),
            _ => text.to_string(),
        }
    }
}

/// `(object)` casts, property reads and `is_wp_error()`, through the helpers `cron.php` declares.
pub struct PhpEventObjects;

impl PhpObjects for PhpEventObjects {
    fn to_object(&self, array: HookValue) -> HookValue {
        call(
            "SpeedySpec\\WpCron\\to_object",
            std::slice::from_ref(&array),
        )
        .unwrap_or(array)
    }

    fn property(&self, object: &HookValue, name: &str) -> Option<HookValue> {
        match call(
            "SpeedySpec\\WpCron\\property",
            &[object.clone(), HookValue::Str(name.into())],
        )?
        .read()
        .into_owned()
        {
            HookValue::Array(items) => match items.as_slice() {
                [(Key::Int(0), HookValue::Bool(true)), (Key::Int(1), value)] => Some(value.clone()),
                _ => None,
            },
            _ => None,
        }
    }

    fn is_wp_error(&self, value: &HookValue) -> bool {
        matches!(
            call(
                "SpeedySpec\\WpCron\\is_wp_error",
                std::slice::from_ref(value)
            ),
            Some(HookValue::Bool(true))
        )
    }
}

/// `microtime( true )`, `time()`, and a real pause.
pub struct SystemClock;

impl Clock for SystemClock {
    fn microtime(&self) -> f64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64())
    }

    fn time(&self) -> i64 {
        self.microtime() as i64
    }

    fn pause(&self, millis: u64) {
        std::thread::sleep(std::time::Duration::from_millis(millis));
    }
}

/// The `doing_cron` transient through WordPress's (native) transient functions; nothing when they are not loaded.
pub struct PhpDoingCron;

impl DoingCron for PhpDoingCron {
    fn get(&self) -> Result<Option<String>, CronError> {
        if !function_exists("get_transient") {
            return Ok(None);
        }
        Ok(
            match call("get_transient", &[HookValue::Str("doing_cron".into())]) {
                Some(HookValue::Str(s)) => Some(s),
                Some(HookValue::Int(n)) => Some(n.to_string()),
                Some(HookValue::Float(f)) => Some(format!("{f:.22}")),
                _ => None,
            },
        )
    }

    fn set(&self, value: &str) -> Result<(), CronError> {
        if function_exists("set_transient") {
            call(
                "set_transient",
                &[
                    HookValue::Str("doing_cron".into()),
                    HookValue::Str(value.into()),
                ],
            );
        }
        Ok(())
    }

    fn delete(&self) -> Result<(), CronError> {
        if function_exists("delete_transient") {
            call("delete_transient", &[HookValue::Str("doing_cron".into())]);
        }
        Ok(())
    }
}

/// `wp-cron.php`'s error reporting, through `cron.php`'s `report_error()` helper.
pub struct PhpErrorReporter;

impl ErrorReporter for PhpErrorReporter {
    fn report(
        &self,
        step: CronStep,
        hook: &HookValue,
        error: &CronResult,
        event: &HookValue,
    ) -> HookValue {
        let step = match step {
            CronStep::Reschedule => "reschedule",
            CronStep::Unschedule => "unschedule",
        };
        call(
            "SpeedySpec\\WpCron\\report_error",
            &[
                HookValue::Str(step.into()),
                hook.clone(),
                crate::natives::marker(error.clone()),
                event.clone(),
            ],
        )
        .unwrap_or(HookValue::Null)
    }
}

/// A PHP call whose exception reaches the caller as a [`CronError`] carrying it.
fn call_or_throw(function: &str, args: &[HookValue]) -> Result<HookValue, CronError> {
    speedyspec_php::call_catching(&HookValue::Str(function.into()), args).map_err(|failure| {
        CronError {
            message: failure.error.to_string(),
            payload: failure
                .exception
                .map(|e| std::rc::Rc::new(e) as std::rc::Rc<dyn std::any::Any>),
        }
    })
}

/// A site id as PHP gives it (an int, or a numeric string from `get_sites()`).
fn site_id(value: &HookValue) -> Option<u64> {
    match &*value.read() {
        HookValue::Int(n) => u64::try_from(*n).ok(),
        HookValue::Str(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// The network option holding the last site whose turn ended.
const CURSOR: &str = "speedyspec_cron_cursor";

/// The network's active sites through `get_sites()`, and the cursor through the site options; a single site when
/// `is_multisite()` is missing or false.
pub struct PhpNetworkSites;

impl NetworkSites for PhpNetworkSites {
    fn is_multisite(&self) -> bool {
        function_exists("is_multisite")
            && function_exists("get_sites")
            && matches!(call("is_multisite", &[]), Some(HookValue::Bool(true)))
    }

    fn site_ids(&self) -> Result<Vec<u64>, CronError> {
        let key = |k: &str| Key::Str(k.into());
        let query = HookValue::Array(vec![
            (key("fields"), HookValue::Str("ids".into())),
            (key("number"), HookValue::Int(0)),
            (key("spam"), HookValue::Int(0)),
            (key("deleted"), HookValue::Int(0)),
            (key("archived"), HookValue::Int(0)),
            (key("orderby"), HookValue::Str("id".into())),
            (key("order"), HookValue::Str("ASC".into())),
        ]);
        Ok(
            match call_or_throw("get_sites", &[query])?.read().into_owned() {
                HookValue::Array(items) => items.iter().filter_map(|(_, v)| site_id(v)).collect(),
                _ => Vec::new(),
            },
        )
    }

    fn cursor(&self) -> Result<Option<u64>, CronError> {
        Ok(site_id(&call_or_throw(
            "get_site_option",
            &[HookValue::Str(CURSOR.into()), HookValue::Bool(false)],
        )?))
    }

    fn set_cursor(&self, site: u64) -> Result<(), CronError> {
        call_or_throw(
            "update_site_option",
            &[HookValue::Str(CURSOR.into()), HookValue::Int(site as i64)],
        )?;
        Ok(())
    }
}

/// `switch_to_blog()` and `restore_current_blog()`.
pub struct PhpSiteSwitcher;

impl SiteSwitcher for PhpSiteSwitcher {
    fn switch_to(&self, site: u64) -> Result<(), CronError> {
        call_or_throw("switch_to_blog", &[HookValue::Int(site as i64)])?;
        Ok(())
    }

    fn restore(&self) -> Result<(), CronError> {
        call_or_throw("restore_current_blog", &[])?;
        Ok(())
    }
}
