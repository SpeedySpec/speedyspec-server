//! Replays what `compat/meta/record.php` recorded from WordPress 7.1.5: `is_protected_meta()` with no filter added,
//! and the `register_meta()` scenarios and `filter_default_metadata()` cases.

mod support;

use speedyspec_php_serialize::PhpValue;
use speedyspec_wp_meta::is_protected_key;
use speedyspec_wp_meta_contracts::MetaKey;

#[test]
fn protected_keys_match_wordpress() {
    let cases = support::cases();
    let protected = cases["protected"].as_array().unwrap();
    assert_eq!(protected.len(), 28);
    for case in protected {
        let key = MetaKey::new(hex::decode(case["key_hex"].as_str().unwrap()).unwrap());
        let PhpValue::Bool(want) = support::decode(&case["result"]) else {
            panic!("result is a bool");
        };
        assert_eq!(
            is_protected_key(&key),
            want,
            "key {} (meta_type {:?})",
            case["key_hex"],
            case["meta_type"]
        );
    }
}

// register_meta(), unregister_meta_key(), registered_meta_key_exists(), get_registered_meta_keys() and
// filter_default_metadata(), as WordPress 7.1.5 answered them under record.php's stubs.

use std::collections::BTreeMap;

use indexmap::IndexMap;
use serde_json::Value;
use speedyspec_php_serialize::PhpKey;
use speedyspec_wp_meta::{
    MetaKeyRegistry, RegisterPorts, Registration, UnfilteredProtection, allowed_args,
    filters_to_remove, prepare_registration, resolve_registration,
};
use speedyspec_wp_meta_contracts::{
    Callables, DefaultSchemaCheck, MetaType, ObjectId, ObjectSubtypes, RevisionSupport,
};

/// `post_type_supports()` as record.php stubs it: true only for `post` and `page`.
struct Revisions;
impl RevisionSupport for Revisions {
    fn supports_revisions(&self, post_type: &str) -> bool {
        matches!(post_type, "post" | "page")
    }
}

/// `rest_validate_value_from_schema()` as record.php stubs it: only the PHP type against `$schema['type']`.
struct Schema;
impl DefaultSchemaCheck for Schema {
    fn default_matches(&self, default: &PhpValue, schema: &PhpValue) -> bool {
        let PhpValue::Array(schema) = schema else {
            return true;
        };
        let kind = schema
            .iter()
            .find(|(k, _)| *k == PhpKey::Str(b"type".to_vec()))
            .map(|(_, v)| v);
        match kind {
            Some(PhpValue::Str(t)) => match t.as_slice() {
                b"string" => matches!(default, PhpValue::Str(_)),
                b"integer" => matches!(default, PhpValue::Int(_)),
                b"number" => matches!(default, PhpValue::Int(_) | PhpValue::Float(_)),
                b"boolean" => matches!(default, PhpValue::Bool(_)),
                b"array" | b"object" => matches!(default, PhpValue::Array(_)),
                _ => true,
            },
            _ => true,
        }
    }
}

/// `is_callable()`: the named functions record.php defines, and WordPress's `__return_true`/`__return_false`.
struct Named;
impl Callables for Named {
    fn is_callable(&self, value: &PhpValue) -> bool {
        matches!(value, PhpValue::Str(name) if matches!(
            name.as_slice(),
            b"speedyspec_sanitize_meta" | b"speedyspec_auth_meta" | b"__return_true" | b"__return_false"
        ))
    }
}

/// `get_object_subtype()` under record.php's `get_post_type()` stub: 10 is a page, 11 a post.
struct Subtypes;
impl ObjectSubtypes for Subtypes {
    fn subtype_of(&self, meta_type: &MetaType, object: ObjectId) -> String {
        match (meta_type.name(), object.0) {
            ("post", 10) => "page".to_owned(),
            ("post", 11) => "post".to_owned(),
            ("comment", _) => "comment".to_owned(),
            _ => String::new(),
        }
    }
}

fn ports() -> RegisterPorts<'static> {
    RegisterPorts {
        revisions: &Revisions,
        schema: &Schema,
        callables: &Named,
        protected: &UnfilteredProtection,
    }
}

fn str_of(value: &PhpValue) -> String {
    match value {
        PhpValue::Str(s) => String::from_utf8(s.clone()).unwrap(),
        other => panic!("expected a string, got {other:?}"),
    }
}

fn bytes_of(value: &PhpValue) -> Vec<u8> {
    match value {
        PhpValue::Str(s) => s.clone(),
        other => panic!("expected a string, got {other:?}"),
    }
}

/// One hooked callback: hook, callback, priority, accepted_args.
type Hooked = (Vec<u8>, PhpValue, i64, u32);

/// One hook's callbacks by priority, each with its accepted_args.
type Priorities = BTreeMap<i64, Vec<(PhpValue, u32)>>;

/// `$wp_filter`, enough of it to diff as record.php does: hooks in insertion order, priorities sorted.
#[derive(Default)]
struct Filters(IndexMap<Vec<u8>, Priorities>);

impl Filters {
    fn add(&mut self, hook: Vec<u8>, callback: PhpValue, priority: i64, accepted: u32) {
        let slot = self.0.entry(hook).or_default().entry(priority).or_default();
        match slot.iter_mut().find(|(cb, _)| *cb == callback) {
            Some(existing) => existing.1 = accepted,
            None => slot.push((callback, accepted)),
        }
    }

    fn has(&self, hook: &[u8], callback: &PhpValue) -> bool {
        self.0.get(hook).is_some_and(|p| {
            p.values()
                .any(|cbs| cbs.iter().any(|(cb, _)| cb == callback))
        })
    }

    /// `remove_filter()` at the default priority 10; an emptied hook leaves `$wp_filter`.
    fn remove(&mut self, hook: &[u8], callback: &PhpValue) {
        if let Some(priorities) = self.0.get_mut(hook) {
            if let Some(cbs) = priorities.get_mut(&10) {
                cbs.retain(|(cb, _)| cb != callback);
                if cbs.is_empty() {
                    priorities.remove(&10);
                }
            }
            if priorities.is_empty() {
                self.0.shift_remove(hook);
            }
        }
    }

    fn snapshot(&self) -> Vec<Hooked> {
        let mut out = Vec::new();
        for (hook, priorities) in &self.0 {
            for (priority, cbs) in priorities {
                for (cb, accepted) in cbs {
                    out.push((hook.clone(), cb.clone(), *priority, *accepted));
                }
            }
        }
        out
    }
}

/// Entries of `a` that `b` lacks, keyed by hook, priority and callback as record.php's snapshot keys are.
fn missing_from(a: &[Hooked], b: &[Hooked]) -> Vec<Hooked> {
    a.iter()
        .filter(|x| !b.iter().any(|y| x.0 == y.0 && x.1 == y.1 && x.2 == y.2))
        .cloned()
        .collect()
}

fn recorded_filters(list: &Value) -> Vec<Hooked> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|f| {
            (
                bytes_of(&support::decode(&f[0])),
                support::decode(&f[1]),
                f[2].as_i64().unwrap(),
                u32::try_from(f[3].as_u64().unwrap()).unwrap(),
            )
        })
        .collect()
}

fn args_array(args: &[(PhpKey, PhpValue)]) -> PhpValue {
    PhpValue::Array(args.to_vec())
}

/// `$wp_meta_keys`, rebuilt from the registry: type, then subtype, then key, each in insertion order.
fn export(registry: &MetaKeyRegistry) -> PhpValue {
    let mut types: Vec<(PhpKey, PhpValue)> = Vec::new();
    for entry in registry.iter() {
        let type_key = PhpKey::Str(entry.object_type.as_bytes().to_vec());
        let sub_key = PhpKey::Str(entry.subtype.as_bytes().to_vec());
        if types.last().map(|(k, _)| k) != Some(&type_key) {
            types.push((type_key, PhpValue::Array(Vec::new())));
        }
        let PhpValue::Array(subs) = &mut types.last_mut().unwrap().1 else {
            unreachable!()
        };
        if subs.last().map(|(k, _)| k) != Some(&sub_key) {
            subs.push((sub_key, PhpValue::Array(Vec::new())));
        }
        let PhpValue::Array(keys) = &mut subs.last_mut().unwrap().1 else {
            unreachable!()
        };
        keys.push((
            PhpKey::Str(entry.key.as_bytes().to_vec()),
            args_array(&entry.args),
        ));
    }
    PhpValue::Array(types)
}

/// Replays one scenario's calls, as the phase-3 adapter will: the domain decides, the replay applies.
#[derive(Default)]
struct Replay {
    registry: MetaKeyRegistry,
    filters: Filters,
    allowed_list: bool,
    initialized: bool,
}

struct Outcome {
    result: PhpValue,
    doing_it_wrong: Vec<(String, String, String)>,
}

impl Replay {
    fn call(&mut self, function: &str, args: &[PhpValue]) -> Outcome {
        let mut doing_it_wrong = Vec::new();
        let result = match function {
            "add_filter" => {
                let hook = bytes_of(&args[0]);
                if hook == b"register_meta_args"
                    && args[1] == PhpValue::Str(b"_wp_register_meta_args_allowed_list".to_vec())
                {
                    self.allowed_list = true;
                }
                let PhpValue::Int(priority) = args[2] else {
                    panic!()
                };
                let PhpValue::Int(accepted) = args[3] else {
                    panic!()
                };
                self.filters.add(
                    hook,
                    args[1].clone(),
                    priority,
                    u32::try_from(accepted).unwrap(),
                );
                PhpValue::Bool(true)
            }
            "register_meta" => {
                self.initialized = true;
                let object_type = str_of(&args[0]);
                let key = MetaKey::new(bytes_of(&args[1]));
                let mut prepared =
                    prepare_registration(args[2].clone(), args.get(3).cloned(), &Named);
                // The register_meta_args filter: only the allowed-list scenario hooks one.
                if self.allowed_list {
                    prepared.args = allowed_args(prepared.args);
                }
                let registration = resolve_registration(&object_type, &key, prepared, &ports());
                let default_hook = format!("default_{object_type}_metadata").into_bytes();
                let default_cb = PhpValue::Str(b"filter_default_metadata".to_vec());
                let apply = |filters: &mut Filters,
                             to_add: &[speedyspec_wp_meta::FilterToAdd],
                             add_default: bool| {
                    for f in to_add {
                        filters.add(
                            f.hook.clone(),
                            f.callback.clone(),
                            f.priority,
                            f.accepted_args,
                        );
                    }
                    if add_default && !filters.has(&default_hook, &default_cb) {
                        filters.add(default_hook.clone(), default_cb.clone(), 10, 5);
                    }
                };
                match registration {
                    Registration::Registered {
                        key,
                        filters,
                        add_default_filter,
                    } => {
                        apply(&mut self.filters, &filters, add_default_filter);
                        self.registry.insert(key);
                        PhpValue::Bool(true)
                    }
                    Registration::HooksOnly {
                        filters,
                        add_default_filter,
                    } => {
                        apply(&mut self.filters, &filters, add_default_filter);
                        PhpValue::Bool(false)
                    }
                    Registration::Rejected {
                        message,
                        version,
                        filters,
                    } => {
                        apply(&mut self.filters, &filters, false);
                        doing_it_wrong.push((
                            "register_meta".to_owned(),
                            message,
                            version.to_owned(),
                        ));
                        PhpValue::Bool(false)
                    }
                }
            }
            "unregister_meta_key" => {
                let object_type = str_of(&args[0]);
                let key = MetaKey::new(bytes_of(&args[1]));
                let subtype = args.get(2).map(str_of).unwrap_or_default();
                match self.registry.remove(&object_type, &key, &subtype) {
                    Some(removed) => {
                        for (hook, callback) in filters_to_remove(&object_type, &removed, &Named) {
                            self.filters.remove(&hook, &callback);
                        }
                        PhpValue::Bool(true)
                    }
                    None => PhpValue::Bool(false),
                }
            }
            "registered_meta_key_exists" => {
                let key = MetaKey::new(bytes_of(&args[1]));
                let subtype = args.get(2).map(str_of).unwrap_or_default();
                PhpValue::Bool(self.registry.exists(&str_of(&args[0]), &key, &subtype))
            }
            "get_registered_meta_keys" => {
                let subtype = args.get(1).map(str_of).unwrap_or_default();
                PhpValue::Array(
                    self.registry
                        .keys(&str_of(&args[0]), &subtype)
                        .into_iter()
                        .map(|r| (PhpKey::Str(r.key.as_bytes().to_vec()), args_array(&r.args)))
                        .collect(),
                )
            }
            other => panic!("no replay for {other}"),
        };
        Outcome {
            result,
            doing_it_wrong,
        }
    }

    /// `$wp_meta_keys`: null until `register_meta()` first runs.
    fn wp_meta_keys(&self) -> PhpValue {
        if self.initialized {
            export(&self.registry)
        } else {
            PhpValue::Null
        }
    }
}

fn call_args(call: &Value) -> Vec<PhpValue> {
    let PhpValue::Array(items) = support::decode(&call["args"]) else {
        panic!("args is an array");
    };
    items.into_iter().map(|(_, v)| v).collect()
}

#[test]
fn register_matches_wordpress() {
    let cases = support::cases();
    let scenarios = cases["register"].as_array().unwrap();
    assert_eq!(scenarios.len(), 21);
    let mut calls_replayed = 0;
    for scenario in scenarios {
        let name = scenario["name"].as_str().unwrap();
        let mut replay = Replay::default();
        for (i, call) in scenario["calls"].as_array().unwrap().iter().enumerate() {
            let function = call["fn"].as_str().unwrap();
            let at = format!("{name} call {i} ({function})");
            let before = replay.filters.snapshot();
            let outcome = replay.call(function, &call_args(call));
            let after = replay.filters.snapshot();
            assert_eq!(
                outcome.result,
                support::decode(&call["result"]),
                "{at}: result"
            );
            let want_wrong: Vec<(String, String, String)> = call["doing_it_wrong"]
                .as_array()
                .unwrap()
                .iter()
                .map(|d| {
                    (
                        d["function"].as_str().unwrap().to_owned(),
                        d["message"].as_str().unwrap().to_owned(),
                        d["version"].as_str().unwrap().to_owned(),
                    )
                })
                .collect();
            assert_eq!(outcome.doing_it_wrong, want_wrong, "{at}: doing_it_wrong");
            assert_eq!(
                missing_from(&after, &before),
                recorded_filters(&call["filters"]),
                "{at}: filters added"
            );
            assert_eq!(
                missing_from(&before, &after),
                recorded_filters(&call["filters_removed"]),
                "{at}: filters removed"
            );
            calls_replayed += 1;
        }
        assert_eq!(
            replay.wp_meta_keys(),
            support::decode(&scenario["wp_meta_keys"]),
            "{name}: $wp_meta_keys"
        );
    }
    assert_eq!(calls_replayed, 41);
}

#[test]
fn defaults_match_wordpress() {
    let cases = support::cases();
    let defaults = cases["default"].as_array().unwrap();
    assert_eq!(defaults.len(), 14);
    for case in defaults {
        let name = case["name"].as_str().unwrap();
        let mut replay = Replay::default();
        for call in case["register"].as_array().unwrap() {
            let outcome = replay.call(call["fn"].as_str().unwrap(), &call_args(call));
            assert_eq!(
                outcome.result,
                support::decode(&call["result"]),
                "{name}: setup"
            );
        }
        let meta_type = MetaType::new(case["meta_type"].as_str().unwrap()).unwrap();
        let object = ObjectId(case["object_id"].as_u64().unwrap());
        if let Some(subtype) = case["subtype"].as_str() {
            assert_eq!(
                Subtypes.subtype_of(&meta_type, object),
                subtype,
                "{name}: subtype stub"
            );
        }
        let key = MetaKey::new(hex::decode(case["key_hex"].as_str().unwrap()).unwrap());
        let value = support::decode(&case["value"]);
        let got = replay
            .registry
            .default_for(
                &meta_type,
                object,
                &key,
                case["single"].as_bool().unwrap(),
                &Subtypes,
            )
            .unwrap_or(value);
        assert_eq!(got, support::decode(&case["result"]), "{name}");
    }
}
