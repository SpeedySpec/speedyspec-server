//! `PhpSettingsStore`: the settings registry's state *is* `$wp_registered_settings` and `$new_allowed_options`
//! (with `$new_whitelist_options` a reference to it), so PHP code reading those globals keeps working. Writes go
//! through small PHP helpers in `options.php`, so PHP's own array semantics (appends, index holes, the reference)
//! hold.

use std::rc::Rc;

use indexmap::IndexMap;
use speedyspec_php_ext::{Key, Value};
use speedyspec_wp_hooks::Callback;
use speedyspec_wp_hooks_php::php_callback;
use speedyspec_wp_options::{SettingsStore, php_empty};

use crate::adapters::call;
use crate::ports::plain;

const HELPERS: &str = "SpeedySpec\\WpOptions\\Php\\";

pub struct PhpSettingsStore;

fn text(k: &Key) -> String {
    match k {
        Key::Int(i) => i.to_string(),
        Key::Str(s) => s.clone(),
    }
}

fn helper(name: &str, args: &[Value]) {
    if let Err(e) = call(&format!("{HELPERS}{name}"), args) {
        eprintln!("speedyspec_wordpress: {name} failed: {e}");
    }
}

/// `$args` with each entry as PHP holds it (a carried `Callback` back to its PHP callable).
fn to_php(args: Value) -> Value {
    match args {
        Value::Array(items) => {
            Value::Array(items.into_iter().map(|(k, v)| (k, plain(v))).collect())
        }
        other => other,
    }
}

impl SettingsStore for PhpSettingsStore {
    fn registered(&self) -> IndexMap<String, Value> {
        match speedyspec_php::global("wp_registered_settings").map(|v| v.read().into_owned()) {
            Some(Value::Array(items)) => items.into_iter().map(|(k, v)| (text(&k), v)).collect(),
            _ => IndexMap::new(),
        }
    }

    /// The setting's args, its `sanitize_callback` as a `Callback` the registry can remove.
    fn get(&self, name: &str) -> Option<Value> {
        let args = self.registered().get(name)?.read().into_owned();
        let Value::Array(items) = args else {
            return Some(args);
        };
        Some(Value::Array(
            items
                .into_iter()
                .map(|(k, v)| {
                    let callback = matches!(&k, Key::Str(s) if s == "sanitize_callback")
                        && !php_empty(&v.read())
                        && !matches!(&v, Value::Opaque(any) if any.is::<Callback>());
                    if callback {
                        (k, Value::Opaque(Rc::new(php_callback(v))))
                    } else {
                        (k, v)
                    }
                })
                .collect(),
        ))
    }

    fn put(&self, name: &str, args: Value) {
        helper("settings_put", &[Value::Str(name.into()), to_php(args)]);
    }

    fn remove(&self, name: &str) {
        helper("settings_remove", &[Value::Str(name.into())]);
    }

    fn allow(&self, group: &str, name: &str) {
        helper(
            "settings_allow",
            &[Value::Str(group.into()), Value::Str(name.into())],
        );
    }

    fn disallow(&self, group: &str, name: &str) {
        helper(
            "settings_disallow",
            &[Value::Str(group.into()), Value::Str(name.into())],
        );
    }
}
