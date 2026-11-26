//! The reading functions: `_get_cron_array()`, `_upgrade_cron_array()`, `wp_get_scheduled_event()`,
//! `wp_next_scheduled()`, `wp_get_schedule()` and `wp_get_ready_cron_jobs()`.

use speedyspec_wp_cron_contracts::{CronArray, CronError};
use speedyspec_wp_hooks_contracts::{HookValue, Key};

use crate::CronApi;
use crate::php::{
    array_cast, array_key, get, is_numeric, isset, key_after, key_value, set3, truthy,
};

fn version_key() -> Key {
    Key::Str("version".into())
}

impl CronApi {
    /// `_get_cron_array()`.
    pub fn get_cron_array(&self) -> Result<CronArray, CronError> {
        let (raw, _) = self.p.store.load()?;
        self.cron_from_option(raw)
    }

    /// The option's value as `_get_cron_array()` turns it into the cron array: non-arrays are empty, a version-less
    /// array is upgraded (and saved), and `version` is dropped.
    pub(crate) fn cron_from_option(&self, raw: HookValue) -> Result<CronArray, CronError> {
        let raw = raw.read().into_owned();
        let HookValue::Array(items) = raw else {
            return Ok(CronArray::default());
        };
        let mut items = if items.iter().any(|(k, _)| *k == version_key()) {
            items
        } else {
            match self.upgrade_cron_array(HookValue::Array(items))? {
                HookValue::Array(items) => items,
                _ => Vec::new(),
            }
        };
        items.retain(|(k, _)| *k != version_key());
        Ok(CronArray::from_value(HookValue::Array(items)))
    }

    /// `_upgrade_cron_array( $cron )`: rekeys a pre-2.1 array by `md5( serialize( $args['args'] ?? array() ) )`
    /// and saves it.
    pub fn upgrade_cron_array(&self, cron: HookValue) -> Result<HookValue, CronError> {
        let cron = cron.read().into_owned();
        if get(&cron, &version_key()) == Some(&HookValue::Int(2)) {
            return Ok(cron);
        }
        let mut new: Vec<(Key, HookValue)> = Vec::new();
        for (timestamp, hooks) in array_cast(&cron) {
            for (hook, args) in array_cast(&hooks) {
                // WordPress 7.1's `$args['args'] ?? array()`.
                let event_args = get(&args, &Key::Str("args".into()))
                    .filter(|v| !matches!(v, HookValue::Null))
                    .cloned()
                    .unwrap_or(HookValue::Array(Vec::new()));
                let key = self.p.keyer.key(&event_args)?;
                set3(
                    &mut new,
                    timestamp.clone(),
                    hook,
                    Key::Str(key.as_str().to_string()),
                    args,
                );
            }
        }
        new.push((version_key(), HookValue::Int(2)));
        let new = HookValue::Array(new);
        self.p.store.save(new.clone())?;
        Ok(new)
    }

    /// `wp_get_scheduled_event( $hook, $args, $timestamp )`.
    pub fn get_scheduled_event(
        &self,
        hook: &HookValue,
        args: &HookValue,
        timestamp: &HookValue,
    ) -> Result<HookValue, CronError> {
        let pre = self.filter(
            "pre_get_scheduled_event",
            HookValue::Null,
            vec![hook.clone(), args.clone(), timestamp.clone()],
        )?;
        if !matches!(*pre.read(), HookValue::Null) {
            return Ok(pre);
        }
        if !matches!(*timestamp.read(), HookValue::Null) && !is_numeric(timestamp) {
            return Ok(HookValue::Bool(false));
        }
        let crons = self.get_cron_array()?;
        if crons.is_empty() {
            return Ok(HookValue::Bool(false));
        }
        let key = Key::Str(self.p.keyer.key(args)?.as_str().to_string());
        let hook_key = array_key(hook)
            .ok_or_else(|| CronError::new("Illegal offset type: a hook must be a scalar"))?;
        let at = |k: &Key| -> Option<HookValue> {
            let cron = crons
                .entries()
                .iter()
                .find(|(ek, _)| ek == k)
                .map(|(_, v)| v)?;
            let event = get(get(cron, &hook_key)?, &key)?;
            (!matches!(event, HookValue::Null)).then(|| event.clone())
        };
        let (timestamp_value, event) = if !truthy(timestamp) {
            let found = crons.entries().iter().find_map(|(k, cron)| {
                get(cron, &hook_key)
                    .is_some_and(|h| isset(h, &key))
                    .then(|| k.clone())
            });
            match found.map(|k| key_value(&k)) {
                Some(next) if truthy(&next) => {
                    let event = at(&array_key(&next).expect("a key")).expect("found above");
                    (next, event)
                }
                _ => return Ok(HookValue::Bool(false)),
            }
        } else {
            let Some(event) = array_key(timestamp).and_then(|k| at(&k)) else {
                return Ok(HookValue::Bool(false));
            };
            (timestamp.clone(), event)
        };
        let mut fields = vec![
            (Key::Str("hook".into()), hook.clone()),
            (Key::Str("timestamp".into()), timestamp_value),
            (
                Key::Str("schedule".into()),
                get(&event, &Key::Str("schedule".into()))
                    .cloned()
                    .unwrap_or(HookValue::Null),
            ),
            (Key::Str("args".into()), args.clone()),
        ];
        if isset(&event, &Key::Str("interval".into())) {
            fields.push((
                Key::Str("interval".into()),
                get(&event, &Key::Str("interval".into()))
                    .cloned()
                    .expect("isset"),
            ));
        }
        Ok(self.p.objects.to_object(HookValue::Array(fields)))
    }

    /// `wp_next_scheduled( $hook, $args )`.
    pub fn next_scheduled(
        &self,
        hook: &HookValue,
        args: &HookValue,
    ) -> Result<HookValue, CronError> {
        let event = self.get_scheduled_event(hook, args, &HookValue::Null)?;
        if !truthy(&event) {
            return Ok(HookValue::Bool(false));
        }
        let timestamp = self
            .p
            .objects
            .property(&event, "timestamp")
            .unwrap_or(HookValue::Null);
        self.filter(
            "wp_next_scheduled",
            timestamp,
            vec![event, hook.clone(), args.clone()],
        )
    }

    /// `wp_get_schedule( $hook, $args )`.
    pub fn get_schedule(&self, hook: &HookValue, args: &HookValue) -> Result<HookValue, CronError> {
        let event = self.get_scheduled_event(hook, args, &HookValue::Null)?;
        let schedule = if truthy(&event) {
            self.p
                .objects
                .property(&event, "schedule")
                .unwrap_or(HookValue::Null)
        } else {
            HookValue::Bool(false)
        };
        self.filter("get_schedule", schedule, vec![hook.clone(), args.clone()])
    }

    /// `wp_get_ready_cron_jobs()`.
    pub fn get_ready_cron_jobs(&self) -> Result<HookValue, CronError> {
        let pre = self.filter("pre_get_ready_cron_jobs", HookValue::Null, Vec::new())?;
        if !matches!(*pre.read(), HookValue::Null) {
            return Ok(pre);
        }
        let crons = self.get_cron_array()?;
        let now = self.p.clock.microtime();
        let mut ready = Vec::new();
        for (timestamp, hooks) in crons.entries() {
            if key_after(timestamp, now) {
                break;
            }
            ready.push((timestamp.clone(), hooks.clone()));
        }
        Ok(HookValue::Array(ready))
    }
}
