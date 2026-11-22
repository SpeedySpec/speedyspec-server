//! The native functions behind `cron.php`. Each takes WordPress's arguments as the wrapper passes them (defaults
//! filled in) and returns WordPress's value, or an error marker the wrapper turns into a `WP_Error`.

use std::rc::Rc;

use speedyspec_hooks::Key;
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_php::native::{self, NativeError};
use speedyspec_wp_cron::{CronApi, truthy};
use speedyspec_wp_cron_contracts::{CronError, CronResult};
use speedyspec_wp_hooks::HookValue;

fn arg(args: &[HookValue], i: usize) -> HookValue {
    args.get(i).cloned().unwrap_or(HookValue::Null)
}

fn flag(args: &[HookValue], i: usize) -> bool {
    args.get(i).is_some_and(truthy)
}

/// A failure the core could not hide, as the native function's: a PHP exception a callback threw, rethrown.
fn native_error(err: CronError) -> NativeError {
    match &err.payload {
        Some(payload) => match payload.downcast_ref::<HookValue>() {
            Some(exception) => NativeError::Rethrow(exception.clone()),
            None => NativeError::Silent,
        },
        None => NativeError::throw("Error", err.message),
    }
}

fn marker(result: CronResult) -> HookValue {
    let s = |v: &str| HookValue::Str(v.into());
    match result {
        CronResult::Value(v) => v,
        CronResult::Error { code, message } => HookValue::Array(vec![
            (Key::Str("__speedyspec_error".into()), HookValue::Bool(true)),
            (Key::Str("code".into()), s(code)),
            (Key::Str("message".into()), HookValue::Str(message)),
        ]),
        CronResult::Merge(errors) => HookValue::Array(vec![
            (Key::Str("__speedyspec_merge".into()), HookValue::Bool(true)),
            (
                Key::Str("errors".into()),
                HookValue::Array(
                    errors
                        .into_iter()
                        .enumerate()
                        .map(|(i, e)| (Key::Int(i as i64), marker(e)))
                        .collect(),
                ),
            ),
        ]),
    }
}

fn answer(result: Result<CronResult, CronError>) -> Result<HookValue, NativeError> {
    result.map(marker).map_err(native_error)
}

fn value(result: Result<HookValue, CronError>) -> Result<HookValue, NativeError> {
    result.map_err(native_error)
}

type Native = Box<dyn Fn(&CronApi, &[HookValue]) -> Result<HookValue, NativeError>>;

pub(crate) fn functions(api: &Rc<CronApi>) -> Vec<FunctionBuilder<'static>> {
    let natives: Vec<(&str, Native)> = vec![
        (
            "wp_schedule_single_event",
            Box::new(|c, a| {
                answer(c.schedule_single_event(&arg(a, 0), &arg(a, 1), &arg(a, 2), flag(a, 3)))
            }),
        ),
        (
            "wp_schedule_event",
            Box::new(|c, a| {
                answer(c.schedule_event(&arg(a, 0), &arg(a, 1), &arg(a, 2), &arg(a, 3), flag(a, 4)))
            }),
        ),
        (
            "wp_reschedule_event",
            Box::new(|c, a| {
                answer(c.reschedule_event(
                    &arg(a, 0),
                    &arg(a, 1),
                    &arg(a, 2),
                    &arg(a, 3),
                    flag(a, 4),
                ))
            }),
        ),
        (
            "wp_unschedule_event",
            Box::new(|c, a| {
                answer(c.unschedule_event(&arg(a, 0), &arg(a, 1), &arg(a, 2), flag(a, 3)))
            }),
        ),
        (
            "wp_clear_scheduled_hook",
            Box::new(|c, a| answer(c.clear_scheduled_hook(&arg(a, 0), &arg(a, 1), flag(a, 2)))),
        ),
        (
            "wp_unschedule_hook",
            Box::new(|c, a| answer(c.unschedule_hook(&arg(a, 0), flag(a, 1)))),
        ),
        (
            "wp_get_scheduled_event",
            Box::new(|c, a| value(c.get_scheduled_event(&arg(a, 0), &arg(a, 1), &arg(a, 2)))),
        ),
        (
            "wp_next_scheduled",
            Box::new(|c, a| value(c.next_scheduled(&arg(a, 0), &arg(a, 1)))),
        ),
        (
            "wp_get_ready_cron_jobs",
            Box::new(|c, _| value(c.get_ready_cron_jobs())),
        ),
        (
            "wp_get_schedules",
            Box::new(|c, _| value(c.get_schedules())),
        ),
        (
            "wp_get_schedule",
            Box::new(|c, a| value(c.get_schedule(&arg(a, 0), &arg(a, 1)))),
        ),
        (
            "_get_cron_array",
            Box::new(|c, _| value(c.get_cron_array().map(|cron| cron.to_value()))),
        ),
        (
            "_set_cron_array",
            Box::new(|c, a| answer(c.set_cron_array(&arg(a, 0), flag(a, 1)))),
        ),
        (
            "_upgrade_cron_array",
            Box::new(|c, a| value(c.upgrade_cron_array(arg(a, 0)))),
        ),
    ];
    natives
        .into_iter()
        .map(|(name, run)| {
            let api = api.clone();
            native::function(
                &format!("SpeedySpec\\WpCron\\Native\\{name}"),
                move |args| run(&api, args),
            )
        })
        .collect()
}
