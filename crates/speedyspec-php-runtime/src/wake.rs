//! The request's next wake time: when the app wants its scheduled call next. The worker puts it in the report it
//! sends after the request (`WorkerReport.next_cron`), and the server's scheduler wakes then. It is generic: the server
//! learns a time, not a WordPress fact. WordPress's cron writes it through [`WakeReporter`]; any PHP app can write it
//! with `SpeedySpec\Runtime\report_next_wake( int $timestamp )`.

use std::cell::Cell;
use std::rc::Rc;

use speedyspec_hooks::Value;
use speedyspec_php::NativeArea;
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_php::native::{self, NativeError};
use speedyspec_wp_cron_contracts::WakeReporter;

/// The earliest wake time reported in the running request.
#[derive(Default)]
pub struct NextWake(Cell<Option<i64>>);

impl NextWake {
    pub fn new() -> Rc<NextWake> {
        Rc::new(NextWake::default())
    }

    /// The earliest time reported since the last take, a time in the past as 0, and empties the slot.
    pub fn take(&self) -> Option<u64> {
        self.0.take().map(|t| u64::try_from(t).unwrap_or(0))
    }
}

impl WakeReporter for NextWake {
    fn wake_at(&self, timestamp: i64) {
        self.0
            .set(Some(self.0.get().map_or(timestamp, |t| t.min(timestamp))));
    }
}

/// `SpeedySpec\Runtime\report_next_wake()`, and the slot emptied at every request start.
pub struct NextWakeArea(pub Rc<NextWake>);

impl NativeArea for NextWakeArea {
    fn functions(&self) -> Vec<FunctionBuilder<'static>> {
        let wake = self.0.clone();
        vec![native::function(
            "SpeedySpec\\Runtime\\report_next_wake",
            move |args: &[Value]| match args.first().map(|v| v.read().into_owned()) {
                Some(Value::Int(timestamp)) => {
                    wake.wake_at(timestamp);
                    Ok(Value::Null)
                }
                Some(Value::Float(timestamp)) => {
                    wake.wake_at(timestamp as i64);
                    Ok(Value::Null)
                }
                other => Err(NativeError::throw(
                    "TypeError",
                    format!(
                        "SpeedySpec\\Runtime\\report_next_wake(): Argument #1 ($timestamp) must be of type int, {} given",
                        match other {
                            None => "none".to_string(),
                            Some(Value::Str(_)) => "string".to_string(),
                            Some(Value::Null) => "null".to_string(),
                            Some(Value::Bool(_)) => "bool".to_string(),
                            Some(Value::Array(_)) => "array".to_string(),
                            Some(_) => "object".to_string(),
                        }
                    ),
                )),
            },
        )]
    }

    fn request_start(&self) {
        self.0.take();
    }
}
