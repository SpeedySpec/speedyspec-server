//! The schema the sanitizer keeps keys by, as a shared, copy-on-write tree.
//!
//! WordPress builds this schema as a PHP array on every construction, copying `VALID_STYLES` and friends into each
//! block's entry. Here subtrees are shared through `Rc` and copied only when changed (`Rc::make_mut`), which keeps
//! PHP's value semantics, including the order-dependent snapshot of the blocks schema taken for each variation.

use std::rc::Rc;

use indexmap::IndexMap;
use speedyspec_hooks::{Key, Value};

/// One schema node: a leaf that keeps any value (`null`), a boolean marker (the value must be a boolean), or an
/// object of allowed keys.
#[derive(Clone, Debug)]
pub enum Spec {
    /// Any value is kept as is.
    Any,
    /// Only a boolean value is kept.
    Bool,
    /// Only an array is kept, filtered by these keys.
    Map(Rc<IndexMap<Key, Spec>>),
}

impl Spec {
    /// An object schema with no keys.
    pub fn empty() -> Spec {
        Spec::Map(Rc::new(IndexMap::new()))
    }

    /// Reads a schema written as WordPress writes one: `null` leaves, boolean markers, nested arrays.
    pub fn from_value(value: &Value) -> Spec {
        match value {
            Value::Bool(_) => Spec::Bool,
            Value::Array(entries) => Spec::Map(Rc::new(
                entries
                    .iter()
                    .map(|(k, v)| (k.clone(), Spec::from_value(v)))
                    .collect(),
            )),
            _ => Spec::Any,
        }
    }

    /// The schema of `key`.
    pub fn get(&self, key: &Key) -> Option<&Spec> {
        match self {
            Spec::Map(m) => m.get(key),
            _ => None,
        }
    }

    fn map_mut(&mut self) -> &mut IndexMap<Key, Spec> {
        if !matches!(self, Spec::Map(_)) {
            *self = Spec::empty();
        }
        match self {
            Spec::Map(m) => Rc::make_mut(m),
            _ => unreachable!(),
        }
    }

    /// `$schema[$key] = $spec`.
    pub fn set(&mut self, key: Key, spec: Spec) {
        self.map_mut().insert(key, spec);
    }

    /// `$schema[$key]`, created as an empty object when missing, for writing.
    pub fn entry(&mut self, key: Key) -> &mut Spec {
        self.map_mut().entry(key).or_insert_with(Spec::empty)
    }

    /// `unset( $schema[$key] )`.
    pub fn remove(&mut self, key: &Key) {
        if matches!(self, Spec::Map(_)) {
            self.map_mut().shift_remove(key);
        }
    }

    /// The object's keys, in order.
    pub fn keys(&self) -> Vec<Key> {
        match self {
            Spec::Map(m) => m.keys().cloned().collect(),
            _ => Vec::new(),
        }
    }
}

/// `WP_Theme_JSON::remove_keys_not_in_schema( $tree, $schema )`.
pub fn remove_keys_not_in_schema(tree: Value, schema: &Spec) -> Value {
    let Value::Array(entries) = tree else {
        return tree;
    };
    let mut out = Vec::with_capacity(entries.len());
    for (key, value) in entries {
        match schema.get(&key) {
            None => {}
            Some(Spec::Bool) => {
                if matches!(value, Value::Bool(_)) {
                    out.push((key, value));
                }
            }
            Some(Spec::Any) => out.push((key, value)),
            Some(child @ Spec::Map(_)) => {
                let Value::Array(items) = value else { continue };
                if items.iter().all(|(k, _)| matches!(k, Key::Int(_))) {
                    let item_schema = match child.get(&Key::Int(0)) {
                        Some(s @ Spec::Map(_)) => Some(s),
                        _ => None,
                    };
                    let items = items
                        .into_iter()
                        .map(|(k, v)| {
                            (
                                k,
                                item_schema.map_or_else(
                                    || v.clone(),
                                    |s| remove_keys_not_in_schema(v.clone(), s),
                                ),
                            )
                        })
                        .collect();
                    out.push((key, Value::Array(items)));
                } else {
                    let kept = remove_keys_not_in_schema(Value::Array(items), child);
                    if !matches!(&kept, Value::Array(e) if e.is_empty()) {
                        out.push((key, kept));
                    }
                }
            }
        }
    }
    Value::Array(out)
}
