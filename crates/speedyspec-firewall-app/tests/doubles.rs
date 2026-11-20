#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Duration;

use speedyspec_firewall::{BanStore, CounterStore, FirewallError, FirewallLog, RawSettings, SettingsSource};

#[derive(Default)]
pub struct MemoryCounters {
    pub counts: RefCell<HashMap<String, u64>>,
}

impl CounterStore for MemoryCounters {
    fn hit(&self, key: &str, _window: Duration) -> Result<u64, FirewallError> {
        let mut counts = self.counts.borrow_mut();
        let count = counts.entry(key.to_string()).or_insert(0);
        *count += 1;
        Ok(*count)
    }
}

#[derive(Default)]
pub struct MemoryBans {
    pub bans: RefCell<HashMap<String, (Duration, String)>>,
}

impl BanStore for MemoryBans {
    fn ban(&self, key: &str, for_: Duration, reason: &str) -> Result<(), FirewallError> {
        self.bans.borrow_mut().insert(key.to_string(), (for_, reason.to_string()));
        Ok(())
    }
    fn banned(&self, key: &str) -> Result<Option<String>, FirewallError> {
        Ok(self.bans.borrow().get(key).map(|(_, reason)| reason.clone()))
    }
    fn lift(&self, key: &str) -> Result<(), FirewallError> {
        self.bans.borrow_mut().remove(key);
        Ok(())
    }
}

pub struct FailingStores;

impl CounterStore for FailingStores {
    fn hit(&self, _key: &str, _window: Duration) -> Result<u64, FirewallError> {
        Err(FirewallError("store down".into()))
    }
}

impl BanStore for FailingStores {
    fn ban(&self, _: &str, _: Duration, _: &str) -> Result<(), FirewallError> {
        Err(FirewallError("store down".into()))
    }
    fn banned(&self, _: &str) -> Result<Option<String>, FirewallError> {
        Err(FirewallError("store down".into()))
    }
    fn lift(&self, _: &str) -> Result<(), FirewallError> {
        Err(FirewallError("store down".into()))
    }
}

#[derive(Default)]
pub struct RecordingLog {
    pub messages: RefCell<Vec<String>>,
}

impl FirewallLog for RecordingLog {
    fn warn(&self, message: &str) {
        self.messages.borrow_mut().push(message.to_string());
    }
}

pub struct FixedSettings {
    pub raw: RefCell<Result<RawSettings, FirewallError>>,
}

impl SettingsSource for FixedSettings {
    fn load(&self) -> Result<RawSettings, FirewallError> {
        self.raw.borrow().clone()
    }
}

pub fn params(client: &str, method: &str, uri: &str, cookie: Option<&str>) -> Vec<(String, String)> {
    let mut params = vec![
        ("REMOTE_ADDR".to_string(), client.to_string()),
        ("REQUEST_METHOD".to_string(), method.to_string()),
        ("REQUEST_URI".to_string(), uri.to_string()),
    ];
    if let Some(cookie) = cookie {
        params.push(("HTTP_COOKIE".to_string(), cookie.to_string()));
    }
    params
}

pub fn facts(client: &str, method: &str, uri: &str) -> speedyspec_firewall::RequestFacts {
    speedyspec_firewall::RequestFacts::from_params(&params(client, method, uri, None)).unwrap()
}
