//! Native `translate_settings_using_i18n_schema()`: the domain's schema rules walked over PHP's own values, so
//! values the schema does not translate (objects included) pass through untouched, and each translation is the
//! native `translate_with_gettext_context()` (its filters included).

use speedyspec_hooks::Key;
use speedyspec_php::ext_php_rs::args::Arg;
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_php::ext_php_rs::flags::DataType;
use speedyspec_php::ext_php_rs::types::{ArrayKey, PropertyQuery, ZendHashTable, Zval};
use speedyspec_php::ext_php_rs::zend::ExecuteData;
use speedyspec_wp_i18n::{I18nError, SettingsTree, translate_settings};
use speedyspec_wp_i18n_contracts::PortError;

use crate::state::shared;
use crate::zv;

/// A PHP value, owned (one reference).
struct Tree(Zval);

impl Clone for Tree {
    fn clone(&self) -> Tree {
        Tree(self.0.shallow_clone())
    }
}

impl Tree {
    fn value(&self) -> &Zval {
        self.0.dereference()
    }
}

fn key_of(k: ArrayKey<'_>) -> Key {
    match k {
        ArrayKey::Long(n) => Key::Int(n),
        ArrayKey::String(s) => Key::Str(s),
        ArrayKey::Str(s) => Key::Str(s.to_owned()),
        ArrayKey::ZendString(s) => Key::Str(String::from_utf8_lossy(s.as_bytes()).into_owned()),
    }
}

impl SettingsTree for Tree {
    fn is_empty(&self) -> bool {
        let v = self.value();
        if v.is_null() {
            return true;
        }
        if let Some(b) = v.bool() {
            return !b;
        }
        if let Some(n) = v.long() {
            return n == 0;
        }
        if let Some(f) = v.double() {
            return f == 0.0;
        }
        if let Some(s) = v.zend_str() {
            let b = s.as_bytes();
            return b.is_empty() || b == b"0";
        }
        if let Some(a) = v.array() {
            return a.len() == 0;
        }
        false
    }

    fn as_str(&self) -> Option<&[u8]> {
        self.value().zend_str().map(|s| s.as_bytes())
    }

    fn is_array(&self) -> bool {
        self.value().is_array()
    }

    fn is_object(&self) -> bool {
        self.value().is_object()
    }

    fn entries(&self) -> Vec<(Key, Tree)> {
        match self.value().array() {
            Some(a) => a
                .iter()
                .map(|(k, v)| (key_of(k), Tree(v.shallow_clone())))
                .collect(),
            None => Vec::new(),
        }
    }

    fn first(&self) -> Option<Tree> {
        self.value()
            .array()
            .and_then(|a| a.get_index(0))
            .map(|v| Tree(v.shallow_clone()))
    }

    fn property(&self, key: &Key) -> Option<Tree> {
        let object = self.value().object()?;
        let name = match key {
            Key::Int(n) => n.to_string(),
            Key::Str(s) => s.clone(),
        };
        if !object
            .has_property(&name, PropertyQuery::Isset)
            .unwrap_or(false)
        {
            return None;
        }
        object
            .get_property::<&Zval>(&name)
            .ok()
            .map(|v| Tree(v.shallow_clone()))
    }

    fn null() -> Tree {
        Tree(Zval::new())
    }

    fn array(entries: Vec<(Key, Tree)>) -> Tree {
        let mut ht = ZendHashTable::new();
        for (k, v) in entries {
            let inserted = match k {
                Key::Int(n) => ht.insert(n, v.0),
                Key::Str(s) => ht.insert(s, v.0),
            };
            debug_assert!(inserted.is_ok());
        }
        let mut z = Zval::new();
        z.set_hashtable(ht);
        Tree(z)
    }
}

/// The error raised when a translation threw: the exception stays pending for PHP.
fn threw() -> I18nError {
    I18nError::Port(PortError::message("a translation threw"))
}

fn run(ex: &ExecuteData) -> Option<Tree> {
    let generation = shared().map_or(0, |s| s.generation.get());
    let translate = zv::function(generation, "translate_with_gettext_context");
    if translate.is_null() {
        return None;
    }
    let schema = Tree(zv::arg_copy(ex, 0)?);
    let settings = Tree(zv::arg_copy(ex, 1).unwrap_or_default());
    let domain = Tree(zv::arg_copy(ex, 2).unwrap_or_default());
    let mut call = |text: &[u8], context: &[u8], domain: &Tree| -> Result<Tree, I18nError> {
        let mut params = [
            zv::string_zval(text),
            zv::string_zval(context),
            domain.0.shallow_clone(),
        ];
        let out = zv::call(
            translate,
            std::ptr::null_mut(),
            params.as_mut_ptr().cast(),
            3,
        );
        if zv::exception_pending() {
            return Err(threw());
        }
        Ok(Tree(out))
    };
    translate_settings(&schema, settings, &domain, &mut call).ok()
}

extern "C" fn native_translate_settings(ex: &mut ExecuteData, ret: &mut Zval) {
    // A panic must not unwind into the engine.
    let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(ex))).unwrap_or(None);
    if let Some(tree) = out {
        zv::return_value(ret, tree.0);
    }
}

pub(crate) fn functions() -> Vec<FunctionBuilder<'static>> {
    vec![
        FunctionBuilder::new(
            "translate_settings_using_i18n_schema",
            native_translate_settings,
        )
        .arg(Arg::new("i18n_schema", DataType::Mixed))
        .arg(Arg::new("settings", DataType::Mixed))
        .arg(Arg::new("textdomain", DataType::Mixed))
        .returns(DataType::Mixed, false, true),
    ]
}
