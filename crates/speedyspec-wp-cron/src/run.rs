//! The runner: `wp-cron.php`'s loop, with an atomic claim in place of the `doing_cron` lock.
//!
//! For each due event the filters run in WordPress's order (`pre_reschedule_event`, then `wp_schedule_event()`'s
//! `pre_schedule_event` and `schedule_event` for a recurring event, then `pre_unschedule_event`), and then one
//! compare-and-swap write adds the next occurrence and removes this one, on condition that this one is still there.
//! If another runner removed it first, this runner skips it and fires nothing more for it. So each due event runs
//! once across runners that never talk to each other. When a plugin keeps events elsewhere (`pre_get_ready_cron_jobs`
//! answered, or `pre_reschedule_event`/`pre_unschedule_event` short-circuited), no claim is possible and the event goes
//! through WordPress's own calls.

use std::cell::Cell;
use std::time::Duration;

use speedyspec_wp_cron_contracts::{CronError, CronResult, CronStep, RunSummary};
use speedyspec_wp_hooks_contracts::{HookName, HookValue, Key};

use crate::CronApi;
use crate::api::hook_error;
use crate::php::{array_cast, get, isset, key_after, key_value, truthy, unset3};

/// What `wp_reschedule_event()` decided for a recurring event, before any write.
enum Reschedule {
    /// A single event: nothing to reschedule.
    Single,
    /// The next occurrence to add (the filtered event and its key).
    Insert(HookValue, String),
    /// What `wp_reschedule_event()` returned without writing (a filter's answer, or an error).
    Done(CronResult),
}

enum EventOutcome {
    Ran,
    ClaimedByOthers,
    /// The claim's write kept conflicting: not run, left for the next call.
    Unclaimed,
}

/// Marks a run in progress for as long as it lives, also when the run fails.
struct Running<'a>(&'a std::cell::Cell<bool>);

impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

fn field(name: &str) -> Key {
    Key::Str(name.into())
}

impl CronApi {
    /// Runs the due events, as `wp-cron.php` does, until none is left or `budget` has passed, and reports when cron is
    /// next due (now, when the budget ran out).
    pub fn run_due_events(&self, budget: Duration) -> Result<RunSummary, CronError> {
        let started = self.p.clock.microtime();
        // The claims' writes leave due events behind for a moment; only the run's own answer is reported.
        self.running.set(true);
        let _running = Running(&self.running);
        let pre = self.filter("pre_get_ready_cron_jobs", HookValue::Null, Vec::new())?;
        let foreign = !matches!(*pre.read(), HookValue::Null);
        let ready: Vec<(Key, HookValue)> = if foreign {
            array_cast(&pre.read())
        } else {
            let crons = self.get_cron_array()?;
            let now = self.p.clock.microtime();
            crons
                .entries()
                .iter()
                .take_while(|(timestamp, _)| !key_after(timestamp, now))
                .cloned()
                .collect()
        };
        let mut summary = RunSummary {
            ran: 0,
            claimed_by_others: 0,
            unclaimed: 0,
            next: None,
        };
        if ready.is_empty() {
            summary.next = self.get_cron_array()?.earliest();
            if let Some(next) = summary.next {
                self.p.wake.wake_at(next);
            }
            return Ok(summary);
        }
        let gmt_time = self.p.clock.microtime();
        self.p
            .doing_cron
            .set(&format!("{:.22}", self.p.clock.microtime()))?;
        let mut stopped = false;
        'events: for (timestamp, hooks) in &ready {
            if key_after(timestamp, gmt_time) {
                break;
            }
            let HookValue::Array(hooks) = hooks else {
                continue;
            };
            for (hook, keys) in hooks {
                let HookValue::Array(keys) = keys else {
                    continue;
                };
                for (key, event) in keys {
                    match self.run_event(timestamp, hook, key, event, foreign)? {
                        EventOutcome::Ran => summary.ran += 1,
                        EventOutcome::ClaimedByOthers => summary.claimed_by_others += 1,
                        EventOutcome::Unclaimed => summary.unclaimed += 1,
                    }
                    if self.p.clock.microtime() - started >= budget.as_secs_f64() {
                        stopped = true;
                        break 'events;
                    }
                }
            }
        }
        self.p.doing_cron.delete()?;
        summary.next = if stopped {
            Some(self.p.clock.time())
        } else {
            self.get_cron_array()?.earliest()
        };
        if let Some(next) = summary.next {
            self.p.wake.wake_at(next);
        }
        Ok(summary)
    }

    /// One due event: decide, claim, report errors, run.
    fn run_event(
        &self,
        timestamp_key: &Key,
        hook_key: &Key,
        event_key: &Key,
        event: &HookValue,
        foreign: bool,
    ) -> Result<EventOutcome, CronError> {
        let timestamp = key_value(timestamp_key);
        let hook = key_value(hook_key);
        let schedule = get(event, &field("schedule"))
            .cloned()
            .unwrap_or(HookValue::Null);
        let args = get(event, &field("args"))
            .cloned()
            .unwrap_or(HookValue::Null);
        if foreign {
            if truthy(&schedule) {
                let result = self.reschedule_event(&timestamp, &schedule, &hook, &args, true)?;
                self.report(CronStep::Reschedule, &hook, &result, event)?;
            }
            let result = self.unschedule_event(&timestamp, &hook, &args, true)?;
            self.report(CronStep::Unschedule, &hook, &result, event)?;
            self.run_action(&hook, &args)?;
            return Ok(EventOutcome::Ran);
        }
        let reschedule = if truthy(&schedule) {
            match self.plan_reschedule(&timestamp, &schedule, &hook, &args, true)? {
                Err(done) => Reschedule::Done(done),
                Ok(next) => match self.plan_schedule_event(&next, &schedule, &hook, &args, true)? {
                    Err(done) => Reschedule::Done(done),
                    Ok((next_event, key)) => Reschedule::Insert(next_event, key),
                },
            }
        } else {
            Reschedule::Single
        };
        // A reschedule that ended without a write is reported now, before the unschedule, as wp-cron.php reports it.
        let reschedule = match reschedule {
            Reschedule::Done(result) => {
                self.report(CronStep::Reschedule, &hook, &result, event)?;
                Reschedule::Single
            }
            other => other,
        };
        // wp_unschedule_event()'s checks, then its filter.
        let unschedule_early = if Self::bad_timestamp(&timestamp) {
            Some(self.error_or_false(
                true,
                "invalid_timestamp",
                "Event timestamp must be a valid Unix timestamp.",
            ))
        } else {
            let pre = self.filter(
                "pre_unschedule_event",
                HookValue::Null,
                vec![
                    timestamp.clone(),
                    hook.clone(),
                    args.clone(),
                    HookValue::Bool(true),
                ],
            )?;
            self.pre_outcome(
                pre,
                true,
                "pre_unschedule_event_false",
                "A plugin prevented the event from being unscheduled.",
            )
        };
        if let Some(unscheduled) = unschedule_early {
            // No claim: the reschedule is written on its own, as WordPress writes it.
            let rescheduled = match reschedule {
                Reschedule::Single => None,
                Reschedule::Done(result) => Some(result),
                Reschedule::Insert(next_event, key) => {
                    Some(Self::saved(self.commit(true, None, &|cron| {
                        self.insert_event(cron, &next_event, &key, true)
                    })?))
                }
            };
            if let Some(result) = rescheduled {
                self.report(CronStep::Reschedule, &hook, &result, event)?;
            }
            self.report(CronStep::Unschedule, &hook, &unscheduled, event)?;
            self.run_action(&hook, &args)?;
            return Ok(EventOutcome::Ran);
        }
        let claimed_by_others = Cell::new(false);
        let written = self.commit(true, None, &|cron| {
            let present = cron
                .entries()
                .iter()
                .find(|(k, _)| k == timestamp_key)
                .and_then(|(_, hooks)| get(hooks, hook_key))
                .is_some_and(|keys| isset(keys, event_key));
            if !present {
                claimed_by_others.set(true);
                return Err(CronResult::Value(HookValue::Bool(false)));
            }
            if let Reschedule::Insert(next_event, key) = &reschedule {
                self.insert_event(cron, next_event, key, true)?;
            }
            let mut items = cron.entries().to_vec();
            unset3(&mut items, timestamp_key, hook_key, event_key);
            *cron = speedyspec_wp_cron_contracts::CronArray::from_value(HookValue::Array(items));
            Ok(())
        })?;
        if claimed_by_others.get() {
            return Ok(EventOutcome::ClaimedByOthers);
        }
        if written.is_err() {
            // Other runners are writing and one of them may claim it: running it unclaimed could run it twice.
            return Ok(EventOutcome::Unclaimed);
        }
        self.run_action(&hook, &args)?;
        Ok(EventOutcome::Ran)
    }

    /// `wp-cron.php`'s handling of a step that returned an error: logged, then its error action.
    fn report(
        &self,
        step: CronStep,
        hook: &HookValue,
        result: &CronResult,
        event: &HookValue,
    ) -> Result<(), CronError> {
        let failed = match result {
            CronResult::Error { .. } | CronResult::Merge(_) => true,
            CronResult::Value(value) => self.p.objects.is_wp_error(value),
        };
        if !failed {
            return Ok(());
        }
        let error = self.p.errors.report(step, hook, result, event);
        let action = match step {
            CronStep::Reschedule => "cron_reschedule_event_error",
            CronStep::Unschedule => "cron_unschedule_event_error",
        };
        self.action(action, vec![error, hook.clone(), event.clone()])
    }

    /// `do_action_ref_array( $hook, $v['args'] )`; arguments that are not an array pass none.
    fn run_action(&self, hook: &HookValue, args: &HookValue) -> Result<(), CronError> {
        let args = match &*args.read() {
            HookValue::Array(items) => items.iter().map(|(_, v)| v.clone()).collect(),
            _ => Vec::new(),
        };
        self.p
            .hooks
            .do_action_ref_array(&HookName::from(hook.clone()), args)
            .map_err(hook_error)
    }
}
