//! The native functions behind `cron.php`. Each takes WordPress's arguments as the wrapper passes them (defaults
//! filled in) and returns WordPress's value, or an error marker the wrapper turns into a `WP_Error`.

use std::rc::Rc;

use speedyspec_hooks::Key;
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_php::native::{self, NativeError};
use std::time::Duration;

use speedyspec_wp_cron::{CronApi, SpawnOutcome, truthy};
use speedyspec_wp_cron_contracts::{CronError, CronResult, SiteRef};
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

pub(crate) fn marker(result: CronResult) -> HookValue {
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
    let mut functions: Vec<FunctionBuilder<'static>> = natives
        .into_iter()
        .map(|(name, run)| {
            let api = api.clone();
            native::function(
                &format!("SpeedySpec\\WpCron\\Native\\{name}"),
                move |args| run(&api, args),
            )
        })
        .collect();
    let spawn = api.clone();
    functions.push(native::function(
        "SpeedySpec\\WpCron\\Native\\spawn_cron",
        move |args| {
            let gmt_time = match args.first().map(|v| v.read().into_owned()) {
                Some(HookValue::Int(n)) => n as f64,
                Some(HookValue::Float(f)) => f,
                Some(HookValue::Str(s)) => s.trim().parse().unwrap_or(0.0),
                _ => 0.0,
            };
            let lock_timeout = match args.get(2).map(|v| v.read().into_owned()) {
                Some(HookValue::Int(n)) => n,
                Some(HookValue::Float(f)) => f as i64,
                _ => 60,
            };
            match spawn
                .spawn_cron(gmt_time, flag(args, 1), lock_timeout)
                .map_err(native_error)?
            {
                SpawnOutcome::No => Ok(HookValue::Bool(false)),
                SpawnOutcome::Spawn { doing_wp_cron } => Ok(HookValue::Str(doing_wp_cron)),
            }
        },
    ));
    let runner = api.clone();
    functions.push(native::function(
        "SpeedySpec\\WpCron\\run_due_events",
        move |args| {
            let budget = match args.first().map(|v| v.read().into_owned()) {
                Some(HookValue::Int(n)) => n as f64,
                Some(HookValue::Float(f)) => f,
                _ => 30.0,
            };
            // A huge or infinite budget is no limit (`from_secs_f64` would panic on it).
            let budget = Duration::try_from_secs_f64(budget.max(0.0)).unwrap_or(Duration::MAX);
            let summary = runner.run_due_events(budget).map_err(native_error)?;
            Ok(HookValue::Array(vec![
                (Key::Str("ran".into()), HookValue::Int(summary.ran as i64)),
                (
                    Key::Str("claimed_by_others".into()),
                    HookValue::Int(summary.claimed_by_others as i64),
                ),
                (
                    Key::Str("unclaimed".into()),
                    HookValue::Int(summary.unclaimed as i64),
                ),
                (
                    Key::Str("next".into()),
                    summary.next.map_or(HookValue::Null, HookValue::Int),
                ),
            ]))
        },
    ));
    let lister = api.clone();
    functions.push(native::function(
        "SpeedySpec\\WpCron\\due_sites",
        move |_args| {
            let sites = lister.due_sites().map_err(native_error)?;
            Ok(HookValue::Array(
                sites
                    .into_iter()
                    .enumerate()
                    .map(|(i, site)| (Key::Int(i as i64), HookValue::Str(follow_up_query(&site))))
                    .collect(),
            ))
        },
    ));
    functions
}

/// The query string of a site's follow-up cron call: `site=<id>&host=<domain>&path=<path>`, each value encoded as
/// `rawurlencode()` does.
pub(crate) fn follow_up_query(site: &SiteRef) -> String {
    format!(
        "site={}&host={}&path={}",
        site.id,
        rawurlencode(&site.domain),
        rawurlencode(&site.path)
    )
}

/// PHP's `rawurlencode()`: every byte but `A-Z a-z 0-9 - _ . ~` as `%XX`.
fn rawurlencode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follow_up_query_encodes_like_rawurlencode() {
        let site = SiteRef {
            id: 7,
            domain: "ex-ample.test".into(),
            path: "/a b/ü/".into(),
        };
        assert_eq!(
            follow_up_query(&site),
            "site=7&host=ex-ample.test&path=%2Fa%20b%2F%C3%BC%2F"
        );
    }
}
