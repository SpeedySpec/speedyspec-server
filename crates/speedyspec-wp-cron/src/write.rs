//! The writing functions, each ending in one compare-and-swap save of the cron array, retried on conflict.
//!
//! Template Method: [`CronApi::commit`] holds the shape every write shares (load, mutate, save if unchanged,
//! reload and mutate again on conflict); each function supplies its validation, filters and mutation.

use std::cell::Cell;

use speedyspec_wp_cron_contracts::{CronArray, CronError, CronResult, SaveOutcome, Version};
use speedyspec_wp_hooks::{HookValue, Key};

use crate::CronApi;
use crate::php::{
    array_key, empty, get, is_numeric, isset, key_after, key_before, key_value, number, set3,
    truthy, unset3,
};

/// Compare-and-swap attempts before a write gives up with `could_not_set`.
const ATTEMPTS: u32 = 5;
const MINUTE: f64 = 60.0;

fn version_key() -> Key {
    Key::Str("version".into())
}

/// `$cron['version'] = 2`: in place when present, otherwise last.
fn with_version(cron: HookValue) -> HookValue {
    let mut items = match cron {
        HookValue::Array(items) => items,
        _ => Vec::new(),
    };
    match items.iter_mut().find(|(k, _)| *k == version_key()) {
        Some((_, v)) => *v = HookValue::Int(2),
        None => items.push((version_key(), HookValue::Int(2))),
    }
    HookValue::Array(items)
}

fn field(name: &str) -> Key {
    Key::Str(name.into())
}

/// The cron array's raw entries, mutated, then the array again.
fn edit(cron: &mut CronArray, f: impl FnOnce(&mut Vec<(Key, HookValue)>)) {
    let mut items = cron.entries().to_vec();
    f(&mut items);
    *cron = CronArray::from_value(HookValue::Array(items));
}

impl CronApi {
    fn invalid_timestamp(&self, wp_error: bool) -> CronResult {
        self.error_or_false(
            wp_error,
            "invalid_timestamp",
            "Event timestamp must be a valid Unix timestamp.",
        )
    }

    pub(crate) fn error_or_false(
        &self,
        wp_error: bool,
        code: &'static str,
        message: &str,
    ) -> CronResult {
        if wp_error {
            CronResult::Error {
                code,
                message: self.tr(message),
            }
        } else {
            CronResult::Value(HookValue::Bool(false))
        }
    }

    pub(crate) fn could_not_set(&self, wp_error: bool) -> CronResult {
        self.error_or_false(
            wp_error,
            "could_not_set",
            "The cron event list could not be saved.",
        )
    }

    /// `! is_numeric( $timestamp ) || $timestamp <= 0`.
    fn bad_timestamp(timestamp: &HookValue) -> bool {
        !is_numeric(timestamp) || number(timestamp).is_none_or(|n| n <= 0.0)
    }

    /// What a `pre_*` filter's answer means: `None` to go on, otherwise the function's return.
    pub(crate) fn pre_outcome(
        &self,
        pre: HookValue,
        wp_error: bool,
        code: &'static str,
        message: &str,
    ) -> Option<CronResult> {
        if matches!(*pre.read(), HookValue::Null) {
            return None;
        }
        if wp_error && matches!(*pre.read(), HookValue::Bool(false)) {
            return Some(self.error_or_false(true, code, message));
        }
        if !wp_error && self.p.objects.is_wp_error(&pre) {
            return Some(CronResult::Value(HookValue::Bool(false)));
        }
        Some(CronResult::Value(pre))
    }

    /// The cron array as `_get_cron_array()` reads it, with the version a save compares.
    pub(crate) fn load_cron(&self) -> Result<(CronArray, Version), CronError> {
        let (raw, version) = self.p.store.load()?;
        // A version-less array is upgraded and saved (`_upgrade_cron_array()`), so the version is read again after
        // it. Otherwise the version is the one read with the array: a second read could see another process's write
        // and let this one overwrite it.
        let upgrades = matches!(&*raw.read(), HookValue::Array(items)
            if !items.iter().any(|(k, _)| *k == version_key()));
        let cron = self.cron_from_option(raw)?;
        if upgrades {
            let (_, version) = self.p.store.load()?;
            return Ok((cron, version));
        }
        Ok((cron, version))
    }

    /// Mutates the cron array and saves it if no one wrote since it was read; on conflict, reads it again and
    /// mutates the fresh copy, up to [`ATTEMPTS`] saves. `Ok(Ok(()))` is a save; `Ok(Err(r))` is the function's
    /// return (`apply` refusing, or `false`/`could_not_set`). A value a filter rewrote on read cannot be compared, so
    /// it is saved unconditionally, as WordPress saves.
    pub(crate) fn commit(
        &self,
        wp_error: bool,
        loaded: Option<(CronArray, Version)>,
        apply: &dyn Fn(&mut CronArray) -> Result<(), CronResult>,
    ) -> Result<Result<(), CronResult>, CronError> {
        let (mut base, mut version) = match loaded {
            Some(loaded) => loaded,
            None => self.load_cron()?,
        };
        let mut attempts = 0;
        loop {
            let mut cron = base.clone();
            if let Err(done) = apply(&mut cron) {
                return Ok(Err(done));
            }
            attempts += 1;
            match self
                .p
                .store
                .save_if(&version, with_version(cron.to_value()))?
            {
                SaveOutcome::Written => return Ok(Ok(())),
                SaveOutcome::Unchanged => return Ok(Err(self.could_not_set(wp_error))),
                SaveOutcome::Conflict if attempts >= ATTEMPTS => {
                    return Ok(Err(self.could_not_set(wp_error)));
                }
                SaveOutcome::Conflict => {
                    self.back_off(attempts);
                    let (raw, fresh_version, stored) = self.p.store.reload()?;
                    let fresh = self.cron_from_option(raw)?;
                    if !stored {
                        let mut cron = fresh;
                        if let Err(done) = apply(&mut cron) {
                            return Ok(Err(done));
                        }
                        return Ok(if self.p.store.save(with_version(cron.to_value()))? {
                            Ok(())
                        } else {
                            Err(self.could_not_set(wp_error))
                        });
                    }
                    base = fresh;
                    version = fresh_version;
                }
            }
        }
    }

    /// A randomised pause after the `attempt`th conflict (about 1–2, 2–4, 4–8 and 8–16 ms), so two writers that
    /// keep colliding drift apart instead of one starving the other.
    fn back_off(&self, attempt: u32) {
        let base = 2u64 << attempt.saturating_sub(1).min(3);
        let jitter = (self.p.clock.microtime().fract() * 1_000_000.0) as u64 % (base / 2).max(1);
        self.p.clock.pause(base / 2 + jitter);
    }

    fn saved(result: Result<(), CronResult>) -> CronResult {
        match result {
            Ok(()) => CronResult::Value(HookValue::Bool(true)),
            Err(done) => done,
        }
    }

    fn event_object(&self, fields: Vec<(&str, HookValue)>) -> HookValue {
        self.p.objects.to_object(HookValue::Array(
            fields.into_iter().map(|(k, v)| (field(k), v)).collect(),
        ))
    }

    fn prop(&self, object: &HookValue, name: &str) -> HookValue {
        self.p
            .objects
            .property(object, name)
            .unwrap_or(HookValue::Null)
    }

    /// Writes `$crons[ $event->timestamp ][ $event->hook ][ $key ] = array( … )` and `uksort`s.
    fn insert_event(
        &self,
        cron: &mut CronArray,
        event: &HookValue,
        key: &str,
        recurring: bool,
    ) -> Result<(), CronResult> {
        let ts = array_key(&self.prop(event, "timestamp")).unwrap_or(Key::Str(String::new()));
        let hook = array_key(&self.prop(event, "hook")).unwrap_or(Key::Str(String::new()));
        let mut stored = vec![
            (field("schedule"), self.prop(event, "schedule")),
            (field("args"), self.prop(event, "args")),
        ];
        if recurring {
            stored.push((field("interval"), self.prop(event, "interval")));
        }
        edit(cron, |items| {
            set3(
                items,
                ts,
                hook,
                Key::Str(key.to_string()),
                HookValue::Array(stored),
            )
        });
        cron.natural_sort();
        Ok(())
    }

    /// `wp_schedule_single_event( $timestamp, $hook, $args, $wp_error )`.
    pub fn schedule_single_event(
        &self,
        timestamp: &HookValue,
        hook: &HookValue,
        args: &HookValue,
        wp_error: bool,
    ) -> Result<CronResult, CronError> {
        if Self::bad_timestamp(timestamp) {
            return Ok(self.invalid_timestamp(wp_error));
        }
        let event = self.event_object(vec![
            ("hook", hook.clone()),
            ("timestamp", timestamp.clone()),
            ("schedule", HookValue::Bool(false)),
            ("args", args.clone()),
        ]);
        let pre = self.filter(
            "pre_schedule_event",
            HookValue::Null,
            vec![event.clone(), HookValue::Bool(wp_error)],
        )?;
        if let Some(done) = self.pre_outcome(
            pre,
            wp_error,
            "pre_schedule_event_false",
            "A plugin prevented the event from being scheduled.",
        ) {
            return Ok(done);
        }
        let loaded = self.load_cron()?;
        let key = self.p.keyer.key(args)?.as_str().to_string();
        let hook_key = array_key(hook);
        let ts = number(timestamp).unwrap_or(0.0);
        let now = self.p.clock.time() as f64;
        let min = if ts < now + 10.0 * MINUTE {
            0.0
        } else {
            ts - 10.0 * MINUTE
        };
        let max = if ts < now {
            now + 10.0 * MINUTE
        } else {
            ts + 10.0 * MINUTE
        };
        let event_key = Key::Str(key.clone());
        let duplicate = |cron: &CronArray| -> bool {
            for (event_ts, entry) in cron.entries() {
                if key_before(event_ts, min) {
                    continue;
                }
                if key_after(event_ts, max) {
                    break;
                }
                if let Some(hook_key) = &hook_key
                    && get(entry, hook_key).is_some_and(|h| isset(h, &event_key))
                {
                    return true;
                }
            }
            false
        };
        let duplicate_error = || {
            self.error_or_false(
                wp_error,
                "duplicate_event",
                "A duplicate event already exists.",
            )
        };
        if duplicate(&loaded.0) {
            return Ok(duplicate_error());
        }
        let event = self.filter("schedule_event", event, Vec::new())?;
        if !truthy(&event) {
            return Ok(self.error_or_false(
                wp_error,
                "schedule_event_false",
                "A plugin disallowed this event.",
            ));
        }
        let result = self.commit(wp_error, Some(loaded), &|cron| {
            if duplicate(cron) {
                return Err(duplicate_error());
            }
            self.insert_event(cron, &event, &key, false)
        })?;
        Ok(Self::saved(result))
    }

    /// `wp_schedule_event( $timestamp, $recurrence, $hook, $args, $wp_error )`.
    pub fn schedule_event(
        &self,
        timestamp: &HookValue,
        recurrence: &HookValue,
        hook: &HookValue,
        args: &HookValue,
        wp_error: bool,
    ) -> Result<CronResult, CronError> {
        if Self::bad_timestamp(timestamp) {
            return Ok(self.invalid_timestamp(wp_error));
        }
        let schedules = self.get_schedules()?;
        let Some(schedule) = array_key(recurrence)
            .and_then(|k| get(&schedules, &k).cloned())
            .filter(|s| !matches!(s, HookValue::Null))
        else {
            return Ok(self.error_or_false(
                wp_error,
                "invalid_schedule",
                "Event schedule does not exist.",
            ));
        };
        let event = self.event_object(vec![
            ("hook", hook.clone()),
            ("timestamp", timestamp.clone()),
            ("schedule", recurrence.clone()),
            ("args", args.clone()),
            (
                "interval",
                get(&schedule, &field("interval"))
                    .cloned()
                    .unwrap_or(HookValue::Null),
            ),
        ]);
        let pre = self.filter(
            "pre_schedule_event",
            HookValue::Null,
            vec![event.clone(), HookValue::Bool(wp_error)],
        )?;
        if let Some(done) = self.pre_outcome(
            pre,
            wp_error,
            "pre_schedule_event_false",
            "A plugin prevented the event from being scheduled.",
        ) {
            return Ok(done);
        }
        let event = self.filter("schedule_event", event, Vec::new())?;
        if !truthy(&event) {
            return Ok(self.error_or_false(
                wp_error,
                "schedule_event_false",
                "A plugin disallowed this event.",
            ));
        }
        let key = self
            .p
            .keyer
            .key(&self.prop(&event, "args"))?
            .as_str()
            .to_string();
        let result = self.commit(wp_error, None, &|cron| {
            self.insert_event(cron, &event, &key, true)
        })?;
        Ok(Self::saved(result))
    }

    /// `wp_reschedule_event( $timestamp, $recurrence, $hook, $args, $wp_error )`.
    pub fn reschedule_event(
        &self,
        timestamp: &HookValue,
        recurrence: &HookValue,
        hook: &HookValue,
        args: &HookValue,
        wp_error: bool,
    ) -> Result<CronResult, CronError> {
        if Self::bad_timestamp(timestamp) {
            return Ok(self.invalid_timestamp(wp_error));
        }
        let schedules = self.get_schedules()?;
        let mut interval = match array_key(recurrence).and_then(|k| get(&schedules, &k).cloned()) {
            Some(schedule) if !matches!(schedule, HookValue::Null) => {
                get(&schedule, &field("interval"))
                    .cloned()
                    .unwrap_or(HookValue::Null)
            }
            _ => HookValue::Int(0),
        };
        if interval == HookValue::Int(0) {
            let scheduled = self.get_scheduled_event(hook, args, timestamp)?;
            if truthy(&scheduled)
                && let Some(found) = self.p.objects.property(&scheduled, "interval")
                && !matches!(found, HookValue::Null)
            {
                interval = found;
            }
        }
        let event = self.event_object(vec![
            ("hook", hook.clone()),
            ("timestamp", timestamp.clone()),
            ("schedule", recurrence.clone()),
            ("args", args.clone()),
            ("interval", interval.clone()),
        ]);
        let pre = self.filter(
            "pre_reschedule_event",
            HookValue::Null,
            vec![event, HookValue::Bool(wp_error)],
        )?;
        if let Some(done) = self.pre_outcome(
            pre,
            wp_error,
            "pre_reschedule_event_false",
            "A plugin prevented the event from being rescheduled.",
        ) {
            return Ok(done);
        }
        if interval == HookValue::Int(0) {
            return Ok(self.error_or_false(
                wp_error,
                "invalid_schedule",
                "Event schedule does not exist.",
            ));
        }
        let now = self.p.clock.time();
        let ts = number(timestamp).unwrap_or(0.0);
        let step = number(&interval).unwrap_or(0.0);
        let next = if ts >= now as f64 {
            match interval {
                HookValue::Int(n) => HookValue::Int(now + n),
                _ => HookValue::Float(now as f64 + step),
            }
        } else {
            // PHP's `%` works on integers.
            let modulus = step.trunc() as i64;
            if modulus == 0 {
                return Err(CronError::new("Modulo by zero"));
            }
            let rest = (now - ts.trunc() as i64) % modulus;
            match interval {
                HookValue::Int(n) => HookValue::Int(now + (n - rest)),
                _ => HookValue::Float(now as f64 + (step - rest as f64)),
            }
        };
        self.schedule_event(&next, recurrence, hook, args, wp_error)
    }

    /// `wp_unschedule_event( $timestamp, $hook, $args, $wp_error )`.
    pub fn unschedule_event(
        &self,
        timestamp: &HookValue,
        hook: &HookValue,
        args: &HookValue,
        wp_error: bool,
    ) -> Result<CronResult, CronError> {
        if Self::bad_timestamp(timestamp) {
            return Ok(self.invalid_timestamp(wp_error));
        }
        let pre = self.filter(
            "pre_unschedule_event",
            HookValue::Null,
            vec![
                timestamp.clone(),
                hook.clone(),
                args.clone(),
                HookValue::Bool(wp_error),
            ],
        )?;
        if let Some(done) = self.pre_outcome(
            pre,
            wp_error,
            "pre_unschedule_event_false",
            "A plugin prevented the event from being unscheduled.",
        ) {
            return Ok(done);
        }
        let key = Key::Str(self.p.keyer.key(args)?.as_str().to_string());
        let ts = array_key(timestamp).unwrap_or(Key::Str(String::new()));
        let hook = array_key(hook).unwrap_or(Key::Str(String::new()));
        let result = self.commit(wp_error, None, &|cron| {
            edit(cron, |items| unset3(items, &ts, &hook, &key));
            Ok(())
        })?;
        Ok(Self::saved(result))
    }

    /// `wp_clear_scheduled_hook( $hook, $args, $wp_error )`, after the PHP wrapper's pre-3.0 argument handling.
    pub fn clear_scheduled_hook(
        &self,
        hook: &HookValue,
        args: &HookValue,
        wp_error: bool,
    ) -> Result<CronResult, CronError> {
        let pre = self.filter(
            "pre_clear_scheduled_hook",
            HookValue::Null,
            vec![hook.clone(), args.clone(), HookValue::Bool(wp_error)],
        )?;
        if let Some(done) = self.pre_outcome(
            pre,
            wp_error,
            "pre_clear_scheduled_hook_false",
            "A plugin prevented the hook from being cleared.",
        ) {
            return Ok(done);
        }
        let crons = self.get_cron_array()?;
        if crons.is_empty() {
            return Ok(CronResult::Value(HookValue::Int(0)));
        }
        let key = Key::Str(self.p.keyer.key(args)?.as_str().to_string());
        let hook_key = array_key(hook);
        let mut results = Vec::new();
        for (timestamp, entry) in crons.entries() {
            if let Some(hook_key) = &hook_key
                && get(entry, hook_key).is_some_and(|h| isset(h, &key))
            {
                results.push(self.unschedule_event(&key_value(timestamp), hook, args, true)?);
            }
        }
        let count = results.len();
        let errors: Vec<CronResult> = results
            .into_iter()
            .filter(|r| match r {
                CronResult::Error { .. } | CronResult::Merge(_) => true,
                CronResult::Value(v) => self.p.objects.is_wp_error(v),
            })
            .collect();
        if !errors.is_empty() {
            return Ok(if wp_error {
                CronResult::Merge(errors)
            } else {
                CronResult::Value(HookValue::Bool(false))
            });
        }
        Ok(CronResult::Value(HookValue::Int(count as i64)))
    }

    /// `wp_unschedule_hook( $hook, $wp_error )`.
    pub fn unschedule_hook(
        &self,
        hook: &HookValue,
        wp_error: bool,
    ) -> Result<CronResult, CronError> {
        let pre = self.filter(
            "pre_unschedule_hook",
            HookValue::Null,
            vec![hook.clone(), HookValue::Bool(wp_error)],
        )?;
        if let Some(done) = self.pre_outcome(
            pre,
            wp_error,
            "pre_unschedule_hook_false",
            "A plugin prevented the hook from being cleared.",
        ) {
            return Ok(done);
        }
        let loaded = self.load_cron()?;
        if loaded.0.is_empty() {
            return Ok(CronResult::Value(HookValue::Int(0)));
        }
        let Some(hook_key) = array_key(hook) else {
            return Ok(CronResult::Value(HookValue::Int(0)));
        };
        let total = Cell::new(0i64);
        let result = self.commit(wp_error, Some(loaded), &|cron| {
            let mut found = Vec::new();
            edit(cron, |items| {
                for (_, entry) in items.iter_mut() {
                    let HookValue::Array(hooks) = entry else {
                        continue;
                    };
                    let current = hooks.iter().find(|(k, _)| *k == hook_key).map(|(_, v)| v);
                    if !empty(current) {
                        found.push(match current {
                            Some(HookValue::Array(events)) => events.len() as i64,
                            _ => 1,
                        });
                    }
                    hooks.retain(|(k, _)| *k != hook_key);
                }
                items.retain(|(_, entry)| !matches!(entry, HookValue::Array(h) if h.is_empty()));
            });
            if found.is_empty() {
                return Err(CronResult::Value(HookValue::Int(0)));
            }
            total.set(found.iter().sum());
            Ok(())
        })?;
        Ok(match result {
            Ok(()) => CronResult::Value(HookValue::Int(total.get())),
            Err(done) => done,
        })
    }

    /// `_set_cron_array( $cron, $wp_error )`: unconditional, as WordPress writes.
    pub fn set_cron_array(
        &self,
        cron: &HookValue,
        wp_error: bool,
    ) -> Result<CronResult, CronError> {
        let cron = match cron.read().into_owned() {
            v @ HookValue::Array(_) => v,
            _ => HookValue::Array(Vec::new()),
        };
        let saved = self.p.store.save(with_version(cron))?;
        Ok(if !saved && wp_error {
            self.could_not_set(true)
        } else {
            CronResult::Value(HookValue::Bool(saved))
        })
    }
}
