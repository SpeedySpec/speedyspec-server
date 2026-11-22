//! `wp_get_schedules()`: data, not a Strategy (every recurrence is computed the same way from its interval).

use speedyspec_wp_cron_contracts::CronError;
use speedyspec_wp_hooks::{HookValue, Key};

use crate::CronApi;

fn schedule(interval: i64, display: String) -> HookValue {
    HookValue::Array(vec![
        (Key::Str("interval".into()), HookValue::Int(interval)),
        (Key::Str("display".into()), HookValue::Str(display)),
    ])
}

/// PHP's `array_merge( $a, $b )`: string keys of `b` replace `a`'s in place, integer keys are appended renumbered.
fn array_merge(a: Vec<(Key, HookValue)>, b: Vec<(Key, HookValue)>) -> Vec<(Key, HookValue)> {
    let mut out: Vec<(Key, HookValue)> = Vec::new();
    let mut next = 0i64;
    for (k, v) in a.into_iter().chain(b) {
        match k {
            Key::Int(_) => {
                out.push((Key::Int(next), v));
                next += 1;
            }
            Key::Str(s) => match out.iter_mut().find(|(ok, _)| *ok == Key::Str(s.clone())) {
                Some((_, existing)) => *existing = v,
                None => out.push((Key::Str(s), v)),
            },
        }
    }
    out
}

impl CronApi {
    /// `wp_get_schedules()`: `array_merge( apply_filters( 'cron_schedules', array() ), $built_in )`, so a filter
    /// adds schedules but cannot change the built-in four.
    pub fn get_schedules(&self) -> Result<HookValue, CronError> {
        let built_in = vec![
            (
                Key::Str("hourly".into()),
                schedule(3600, self.tr("Once Hourly")),
            ),
            (
                Key::Str("twicedaily".into()),
                schedule(43200, self.tr("Twice Daily")),
            ),
            (
                Key::Str("daily".into()),
                schedule(86400, self.tr("Once Daily")),
            ),
            (
                Key::Str("weekly".into()),
                schedule(604800, self.tr("Once Weekly")),
            ),
        ];
        let filtered = self.filter("cron_schedules", HookValue::Array(Vec::new()), Vec::new())?;
        let filtered = match filtered.read().into_owned() {
            HookValue::Array(items) => items,
            _ => Vec::new(),
        };
        Ok(HookValue::Array(array_merge(filtered, built_in)))
    }
}
