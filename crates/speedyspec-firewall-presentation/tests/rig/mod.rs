#![allow(dead_code)]

use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_cache_memory::MemoryCache;
use speedyspec_firewall::FirewallLog;
use speedyspec_firewall_presentation::{FirewallComposition, FirewallScope, FirewallStage};
use speedyspec_native_stage::{NativeStage, StageOutcome, StageRequest};
use speedyspec_wp_hooks::{WpHookApi, IgnoreDeprecations};
use speedyspec_wp_hooks_contracts::HookValue;
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_object_cache::{CurrentObjectCache, ObjectCachePorts};
use speedyspec_wp_options::{Options, OptionsApi, OptionsDeps};
use speedyspec_wp_options_memory::MemoryOptionRepository;

#[derive(Default)]
pub struct RecordingLog {
    pub messages: RefCell<Vec<String>>,
}

impl FirewallLog for RecordingLog {
    fn warn(&self, message: &str) {
        self.messages.borrow_mut().push(message.to_string());
    }
}

pub struct Rig {
    pub hooks: Rc<WpHookApi>,
    pub options: Rc<OptionsApi>,
    pub log: Rc<RecordingLog>,
    pub stage: Rc<FirewallStage>,
}

impl Rig {
    pub fn new(settings: &[(&str, HookValue)]) -> Rig {
        let hooks = Rc::new(WpHookApi::new(
            Rc::new(MemoryHookStore::new()),
            Rc::new(IgnoreDeprecations),
        ));
        let object_cache = Rc::new(CurrentObjectCache::new(ObjectCachePorts::rust(), None));
        let options = Rc::new(OptionsApi::new(OptionsDeps::rust(
            hooks.clone(),
            object_cache,
            Rc::new(MemoryOptionRepository::new()),
        )));
        options
            .update_option(
                "speedyspec_firewall_enabled",
                HookValue::Bool(true),
                HookValue::Null,
            )
            .unwrap();
        for (name, value) in settings {
            options
                .update_option(
                    &format!("speedyspec_firewall_{name}"),
                    value.clone(),
                    HookValue::Null,
                )
                .unwrap();
        }
        let log = Rc::new(RecordingLog::default());
        let stage = FirewallComposition::build(
            Options::new(options.clone()),
            Rc::new(MemoryCache::new()),
            &hooks,
            FirewallScope::Public,
            log.clone(),
        )
        .unwrap();
        Rig {
            hooks,
            options,
            log,
            stage,
        }
    }

    pub fn answer(&self, params: &[(&str, &str)]) -> StageOutcome {
        let params: Vec<(String, String)> = params
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        self.stage.answer(&StageRequest { params: &params })
    }
}

pub fn request(client: &str, method: &str, uri: &str) -> Vec<(&'static str, String)> {
    vec![
        ("REMOTE_ADDR", client.to_string()),
        ("REQUEST_METHOD", method.to_string()),
        ("REQUEST_URI", uri.to_string()),
    ]
}

pub fn answer(rig: &Rig, client: &str, method: &str, uri: &str) -> StageOutcome {
    let owned = request(client, method, uri);
    let params: Vec<(&str, &str)> = owned.iter().map(|(k, v)| (*k, v.as_str())).collect();
    rig.answer(&params)
}
