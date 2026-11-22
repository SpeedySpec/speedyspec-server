//! The cron ports' PHP adapters.

use speedyspec_hooks::Key;
use speedyspec_wp_cron::{RustArgsKeyer, md5_hex};
use speedyspec_wp_cron_contracts::{ArgsKeyer, Clock, CronError, EventKey, PhpObjects, Translator};
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
        call("SpeedySpec\\WpCron\\to_object", std::slice::from_ref(&array)).unwrap_or(array)
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
