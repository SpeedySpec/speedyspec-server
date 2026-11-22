//! The cron array: `[ timestamp => [ hook => [ md5( serialize( args ) ) => event ] ] ]`, without `version`.

use speedyspec_hooks::{Key, Value};

use crate::natsort::strnatcasecmp;

/// The key PHP gives a string used as an array key: a canonical decimal integer that fits becomes an integer.
pub fn key_for(text: &str) -> Key {
    let digits = text.strip_prefix('-').unwrap_or(text);
    let canonical = !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && (digits == "0" || !digits.starts_with('0'))
        && text != "-0";
    match canonical.then(|| text.parse::<i64>().ok()).flatten() {
        Some(n) => Key::Int(n),
        None => Key::Str(text.to_string()),
    }
}

fn key_text(key: &Key) -> String {
    match key {
        Key::Int(n) => n.to_string(),
        Key::Str(s) => s.clone(),
    }
}

/// An event's key under its hook: `md5( serialize( $args ) )` for events WordPress writes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EventKey(String);

impl EventKey {
    /// A key as WordPress computes it: 32 lowercase hexadecimal characters.
    pub fn parse(text: &str) -> Option<EventKey> {
        (text.len() == 32 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
            .then(|| EventKey(text.to_string()))
    }

    /// A key as found in the option, whatever its shape (a plugin may have written any).
    pub fn found(text: String) -> EventKey {
        EventKey(text)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An event's stored fields. `schedule: None` is PHP's `false` (a single event).
#[derive(Debug, Clone, PartialEq)]
pub struct EventData {
    pub schedule: Option<String>,
    pub interval: Option<i64>,
    pub args: Value,
}

impl EventData {
    /// The stored array, in WordPress's key order: `schedule`, `args`, then `interval` for recurring events.
    pub fn to_value(&self) -> Value {
        let mut fields = vec![
            (
                Key::Str("schedule".into()),
                self.schedule
                    .as_ref()
                    .map_or(Value::Bool(false), |s| Value::Str(s.clone())),
            ),
            (Key::Str("args".into()), self.args.clone()),
        ];
        if let Some(interval) = self.interval {
            fields.push((Key::Str("interval".into()), Value::Int(interval)));
        }
        Value::Array(fields)
    }

    fn from_value(value: &Value) -> EventData {
        let field = |name: &str| match value {
            Value::Array(items) => items
                .iter()
                .find(|(k, _)| *k == Key::Str(name.into()))
                .map(|(_, v)| v.clone()),
            _ => None,
        };
        EventData {
            schedule: match field("schedule") {
                Some(Value::Str(s)) => Some(s),
                _ => None,
            },
            interval: match field("interval") {
                Some(Value::Int(n)) => Some(n),
                _ => None,
            },
            args: field("args").unwrap_or(Value::Array(Vec::new())),
        }
    }
}

/// One event, where it sits in the array.
#[derive(Debug, Clone, PartialEq)]
pub struct ScheduledEvent {
    pub hook: String,
    pub timestamp: i64,
    pub key: EventKey,
    pub data: EventData,
}

/// The cron array, in PHP's order. Entries that are not events (a non-numeric key, a hook whose value is not an
/// array, junk a plugin wrote) are kept where they are and written back unchanged.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CronArray {
    entries: Vec<(Key, Value)>,
}

fn find<'a>(items: &'a [(Key, Value)], key: &Key) -> Option<&'a Value> {
    items.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

fn find_mut<'a>(items: &'a mut [(Key, Value)], key: &Key) -> Option<&'a mut Value> {
    items.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v)
}

/// `$items[$key]` as an array to write into, created (or replacing a scalar) as PHP's nested assignment does.
fn slot(items: &mut Vec<(Key, Value)>, key: Key) -> &mut Vec<(Key, Value)> {
    let index = match items.iter().position(|(k, _)| *k == key) {
        Some(i) => i,
        None => {
            items.push((key, Value::Array(Vec::new())));
            items.len() - 1
        }
    };
    if !matches!(items[index].1, Value::Array(_)) {
        items[index].1 = Value::Array(Vec::new());
    }
    match &mut items[index].1 {
        Value::Array(inner) => inner,
        _ => unreachable!("made an array above"),
    }
}

/// PHP's `empty()` of an array entry.
fn empty(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) | Some(Value::Bool(false)) => true,
        Some(Value::Array(items)) => items.is_empty(),
        Some(Value::Int(0)) => true,
        Some(Value::Float(f)) => *f == 0.0,
        Some(Value::Str(s)) => s.is_empty() || s == "0",
        _ => false,
    }
}

impl CronArray {
    /// The option's value without `version`; anything but an array is empty, as `_get_cron_array()` returns.
    pub fn from_value(value: Value) -> CronArray {
        match value {
            Value::Array(entries) => CronArray { entries },
            _ => CronArray::default(),
        }
    }

    pub fn to_value(&self) -> Value {
        Value::Array(self.entries.clone())
    }

    /// The raw entries, for callers that walk the array as PHP's `foreach` does.
    pub fn entries(&self) -> &[(Key, Value)] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Every event, in array order: integer keys whose value is an array of hooks, each an array of events.
    pub fn events(&self) -> Vec<ScheduledEvent> {
        let mut events = Vec::new();
        for (key, hooks) in &self.entries {
            let (Key::Int(timestamp), Value::Array(hooks)) = (key, hooks) else {
                continue;
            };
            for (hook, keyed) in hooks {
                let Value::Array(keyed) = keyed else {
                    continue;
                };
                for (event_key, event) in keyed {
                    if !matches!(event, Value::Array(_)) {
                        continue;
                    }
                    events.push(ScheduledEvent {
                        hook: key_text(hook),
                        timestamp: *timestamp,
                        key: EventKey::found(key_text(event_key)),
                        data: EventData::from_value(event),
                    });
                }
            }
        }
        events
    }

    /// `isset( $crons[ $timestamp ][ $hook ][ $key ] )`.
    pub fn contains(&self, timestamp: i64, hook: &str, key: &EventKey) -> bool {
        let Some(Value::Array(hooks)) = find(&self.entries, &Key::Int(timestamp)) else {
            return false;
        };
        let Some(Value::Array(keyed)) = find(hooks, &key_for(hook)) else {
            return false;
        };
        find(keyed, &key_for(key.as_str())).is_some_and(|v| !matches!(v, Value::Null))
    }

    /// `$crons[ $timestamp ][ $hook ][ $key ] = $event`.
    pub fn insert(&mut self, event: ScheduledEvent) {
        let hooks = slot(&mut self.entries, Key::Int(event.timestamp));
        let keyed = slot(hooks, key_for(&event.hook));
        let key = key_for(event.key.as_str());
        let value = event.data.to_value();
        match find_mut(keyed, &key) {
            Some(existing) => *existing = value,
            None => keyed.push((key, value)),
        }
    }

    /// `wp_unschedule_event()`'s unset, then the hook and the timestamp unset when `empty()`.
    pub fn remove(&mut self, timestamp: i64, hook: &str, key: &EventKey) {
        let ts = Key::Int(timestamp);
        let hook_key = key_for(hook);
        if let Some(Value::Array(hooks)) = find_mut(&mut self.entries, &ts) {
            if let Some(Value::Array(keyed)) = find_mut(hooks, &hook_key) {
                let event_key = key_for(key.as_str());
                keyed.retain(|(k, _)| *k != event_key);
            }
            if empty(find(hooks, &hook_key)) {
                hooks.retain(|(k, _)| *k != hook_key);
            }
        }
        if empty(find(&self.entries, &ts)) {
            self.entries.retain(|(k, _)| *k != ts);
        }
    }

    /// `uksort( $crons, 'strnatcasecmp' )`: stable, as PHP 8's sort is.
    pub fn natural_sort(&mut self) {
        self.entries.sort_by(|(a, _), (b, _)| {
            strnatcasecmp(key_text(a).as_bytes(), key_text(b).as_bytes())
        });
    }

    /// The smallest timestamp key, if any: when the next event may be due.
    pub fn earliest(&self) -> Option<i64> {
        self.entries
            .iter()
            .filter_map(|(k, _)| match k {
                Key::Int(n) => Some(*n),
                Key::Str(_) => None,
            })
            .min()
    }
}
