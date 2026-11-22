//! Which hook store the Hook API uses: the one bound in the container now (the worker's memory store, or a PHP
//! store bound in `app.<id>.php.bindings`), and the PHP store's adapter.

use std::rc::{Rc, Weak};

use speedyspec_container::Container;
use speedyspec_php_ext::{Key, PhpObject, Value, php_port};
use speedyspec_wp_hooks::{
    ArrayKey, Count, DeprecatedHooks, HookError, HookKey, HookStore, HookValue, Partition, WpHook,
};

use crate::registry;

/// The container key of the hook store port.
pub const HOOK_STORE: &str = "SpeedySpec\\WpHooks\\HookStore";

/// The store bound under [`HOOK_STORE`] at the moment of each call, so a PHP binding takes every registration.
///
/// The container is held weakly. The Hook API's handlers are bound in it, and they hold the Hook API, which holds
/// this store, so a strong reference would make a cycle (Container → handler → Hook API → store → Container). When
/// the container is gone, `fallback` answers.
pub struct ResolvedHookStore {
    container: Weak<Container>,
    fallback: Rc<dyn HookStore>,
}

impl ResolvedHookStore {
    /// The store bound in `container` (without keeping it alive), or `fallback` when none is bound.
    pub fn new(container: &Rc<Container>, fallback: Rc<dyn HookStore>) -> ResolvedHookStore {
        ResolvedHookStore {
            container: Rc::downgrade(container),
            fallback,
        }
    }

    fn current(&self) -> Rc<dyn HookStore> {
        self.container
            .upgrade()
            .and_then(|container| container.make_port::<dyn HookStore>(HOOK_STORE).ok())
            .unwrap_or_else(|| self.fallback.clone())
    }
}

impl HookStore for ResolvedHookStore {
    fn hook(&self, name: &HookKey) -> Option<WpHook> {
        self.current().hook(name)
    }
    fn has_hook(&self, name: &HookKey) -> bool {
        self.current().has_hook(name)
    }
    fn hooks(&self) -> Vec<(ArrayKey, WpHook)> {
        self.current().hooks()
    }
    fn counts(&self, kind: Count) -> Vec<(ArrayKey, i64)> {
        self.current().counts(kind)
    }
    fn count(&self, kind: Count, name: &HookKey) -> Option<i64> {
        self.current().count(kind, name)
    }
    fn current_stack(&self) -> Vec<HookValue> {
        self.current().current_stack()
    }
    fn partition(&self) -> Partition {
        self.current().partition()
    }
    fn insert(&self, name: HookKey, hook: WpHook) {
        self.current().insert(name, hook)
    }
    fn remove(&self, name: &HookKey) {
        self.current().remove(name)
    }
    fn clear(&self) {
        self.current().clear()
    }
    fn push_current(&self, name: HookValue) {
        self.current().push_current(name)
    }
    fn pop_current(&self) {
        self.current().pop_current()
    }
    fn set_current_stack(&self, stack: Vec<HookValue>) {
        self.current().set_current_stack(stack)
    }
    fn increment_did(&self, kind: Count, name: &HookKey) {
        self.current().increment_did(kind, name)
    }
    fn set_count(&self, kind: Count, name: &HookKey, value: Option<i64>) {
        self.current().set_count(kind, name, value)
    }
}

/// The hook store as PHP implements it: hooks are `WP_Hook` objects, counts and the stack are arrays.
#[php_port(name = "SpeedySpec\\WpHooks\\HookStorage")]
pub trait HookStorage {
    /// `$wp_filter[$name]`, a `WP_Hook` or null.
    fn hook(&self, name: Value) -> Value;
    /// `$wp_filter`.
    fn hooks(&self) -> Value;
    fn insert(&self, name: Value, hook: Value);
    fn remove(&self, name: Value);
    fn clear(&self);
    /// `$wp_actions` (`kind` "actions") or `$wp_filters` ("filters").
    fn counts(&self, kind: String) -> Value;
    /// Sets a count, or unsets it when `value` is null.
    fn set_count(&self, kind: String, name: Value, value: Value);
    /// `$wp_current_filter`.
    fn current_stack(&self) -> Value;
    fn set_current_stack(&self, stack: Value);
}

/// A PHP implementation of the hook store (bound under [`HOOK_STORE`] in `app.<id>.php.bindings`).
pub struct PhpHookStore(PhpHookStorage);

impl PhpHookStore {
    pub fn new(object: PhpObject) -> PhpHookStore {
        PhpHookStore(PhpHookStorage::new(object))
    }
}

fn kind_name(kind: Count) -> String {
    match kind {
        Count::Actions => "actions".into(),
        Count::Filters => "filters".into(),
    }
}

fn key_of(key: &Key) -> ArrayKey {
    match key {
        Key::Int(n) => ArrayKey::Int(*n),
        Key::Str(s) => ArrayKey::Str(s.clone()),
    }
}

fn entries(value: Value) -> Vec<(Key, Value)> {
    match value {
        Value::Array(entries) => entries,
        _ => Vec::new(),
    }
}

impl HookStore for PhpHookStore {
    fn hook(&self, name: &HookKey) -> Option<WpHook> {
        registry::hook_of(&self.0.hook(name.key().to_value()))
    }
    fn hooks(&self) -> Vec<(ArrayKey, WpHook)> {
        entries(self.0.hooks())
            .into_iter()
            .filter_map(|(k, v)| registry::hook_of(&v).map(|hook| (key_of(&k), hook)))
            .collect()
    }
    fn counts(&self, kind: Count) -> Vec<(ArrayKey, i64)> {
        entries(self.0.counts(kind_name(kind)))
            .into_iter()
            .filter_map(|(k, v)| match v {
                Value::Int(n) => Some((key_of(&k), n)),
                _ => None,
            })
            .collect()
    }
    fn count(&self, kind: Count, name: &HookKey) -> Option<i64> {
        self.counts(kind)
            .into_iter()
            .find(|(k, _)| k == name.key())
            .map(|(_, n)| n)
    }
    fn current_stack(&self) -> Vec<HookValue> {
        entries(self.0.current_stack())
            .into_iter()
            .map(|(_, v)| v)
            .collect()
    }
    fn partition(&self) -> Partition {
        Partition::Request
    }
    fn insert(&self, name: HookKey, hook: WpHook) {
        if let Ok(object) = registry::object_for(&hook) {
            self.0.insert(name.key().to_value(), object);
        }
    }
    fn remove(&self, name: &HookKey) {
        self.0.remove(name.key().to_value())
    }
    fn clear(&self) {
        self.0.clear()
    }
    fn push_current(&self, name: HookValue) {
        let mut stack = self.current_stack();
        stack.push(name);
        self.set_current_stack(stack);
    }
    fn pop_current(&self) {
        let mut stack = self.current_stack();
        stack.pop();
        self.set_current_stack(stack);
    }
    fn set_current_stack(&self, stack: Vec<HookValue>) {
        let list = stack
            .into_iter()
            .enumerate()
            .map(|(i, v)| (Key::Int(i as i64), v))
            .collect();
        self.0.set_current_stack(Value::Array(list))
    }
    fn increment_did(&self, kind: Count, name: &HookKey) {
        let next = self.count(kind, name).unwrap_or(0) + 1;
        self.0
            .set_count(kind_name(kind), name.key().to_value(), Value::Int(next))
    }
    fn set_count(&self, kind: Count, name: &HookKey, value: Option<i64>) {
        self.0.set_count(
            kind_name(kind),
            name.key().to_value(),
            value.map_or(Value::Null, Value::Int),
        )
    }
}

/// Reports deprecated hooks through WordPress's `_deprecated_hook()`, when PHP has one.
pub struct PhpDeprecations;

impl DeprecatedHooks for PhpDeprecations {
    fn deprecated_hook(
        &self,
        hook: &HookValue,
        version: &HookValue,
        replacement: &HookValue,
        message: &HookValue,
    ) -> Result<(), HookError> {
        let exists = speedyspec_php::call(
            &Value::Str("function_exists".into()),
            &[Value::Str("_deprecated_hook".into())],
        );
        if !matches!(exists, Ok(Value::Bool(true))) {
            return Ok(());
        }
        let args = [
            hook.clone(),
            version.clone(),
            replacement.clone(),
            message.clone(),
        ];
        speedyspec_php::call_catching(&Value::Str("_deprecated_hook".into()), &args)
            .map(|_| ())
            .map_err(crate::callback::hook_error)
    }
}
