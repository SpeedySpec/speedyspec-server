//! `translate_settings_using_i18n_schema()`: translating a block's or theme's settings by its i18n schema (an
//! Interpreter over the schema tree).
//!
//! The rules work on any [`SettingsTree`], so the PHP adapter walks zvals in place (objects in the settings pass
//! through untouched) and Rust callers use [`SchemaValue`].

use speedyspec_hooks::Key;
use speedyspec_wp_i18n_contracts::PhpValue;

use crate::I18nError;

/// A PHP value as the schema rules see it.
pub trait SettingsTree: Sized + Clone {
    /// PHP's `empty()`.
    fn is_empty(&self) -> bool;
    /// The bytes, when `is_string()`.
    fn as_str(&self) -> Option<&[u8]>;
    /// `is_array()`.
    fn is_array(&self) -> bool;
    /// `is_object()`.
    fn is_object(&self) -> bool;
    /// An array's entries, in order (only asked of arrays).
    fn entries(&self) -> Vec<(Key, Self)>;
    /// `$array[0]`, `None` when missing (only asked of arrays).
    fn first(&self) -> Option<Self>;
    /// `$object->$key`, `None` unless `isset()` (only asked of objects).
    fn property(&self, key: &Key) -> Option<Self>;
    /// `null`.
    fn null() -> Self;
    /// A new array with these entries.
    fn array(entries: Vec<(Key, Self)>) -> Self;
}

/// `translate_settings_using_i18n_schema( $i18n_schema, $settings, $textdomain )`. `translate` is
/// `translate_with_gettext_context( $text, $context, $textdomain )`.
pub fn translate_settings<T: SettingsTree>(
    schema: &T,
    settings: T,
    domain: &T,
    translate: &mut dyn FnMut(&[u8], &[u8], &T) -> Result<T, I18nError>,
) -> Result<T, I18nError> {
    if schema.is_empty() || settings.is_empty() || domain.is_empty() {
        return Ok(settings);
    }
    if let (Some(context), Some(text)) = (schema.as_str(), settings.as_str()) {
        return translate(text, context, domain);
    }
    if schema.is_array() && settings.is_array() {
        let item_schema = schema.first().unwrap_or_else(T::null);
        let mut out = Vec::new();
        for (i, (_, value)) in settings.entries().into_iter().enumerate() {
            out.push((
                Key::Int(i as i64),
                translate_settings(&item_schema, value, domain, translate)?,
            ));
        }
        return Ok(T::array(out));
    }
    if schema.is_object() && settings.is_array() {
        let star = Key::Str("*".into());
        let mut out = Vec::new();
        for (key, value) in settings.entries() {
            let translated = match schema.property(&key).or_else(|| schema.property(&star)) {
                Some(item_schema) => translate_settings(&item_schema, value, domain, translate)?,
                None => value,
            };
            out.push((key, translated));
        }
        return Ok(T::array(out));
    }
    Ok(settings)
}

/// Settings and schemas for Rust callers: PHP's arrays and objects kept apart, other values carried.
#[derive(Clone, Debug, PartialEq)]
pub enum SchemaValue {
    Null,
    Str(Vec<u8>),
    /// A PHP array.
    List(Vec<(Key, SchemaValue)>),
    /// A PHP object's public properties (a decoded JSON object).
    Object(Vec<(String, SchemaValue)>),
    /// Any other value, carried unchanged.
    Other(PhpValue),
}

fn key_name(key: &Key) -> String {
    match key {
        Key::Int(n) => n.to_string(),
        Key::Str(s) => s.clone(),
    }
}

impl SettingsTree for SchemaValue {
    fn is_empty(&self) -> bool {
        match self {
            SchemaValue::Null => true,
            SchemaValue::Str(s) => s.is_empty() || s.as_slice() == b"0",
            SchemaValue::List(items) => items.is_empty(),
            SchemaValue::Object(_) => false,
            SchemaValue::Other(v) => !speedyspec_wp_i18n_contracts::php_truthy(v),
        }
    }

    fn as_str(&self) -> Option<&[u8]> {
        match self {
            SchemaValue::Str(s) => Some(s),
            _ => None,
        }
    }

    fn is_array(&self) -> bool {
        matches!(self, SchemaValue::List(_))
    }

    fn is_object(&self) -> bool {
        matches!(self, SchemaValue::Object(_))
    }

    fn entries(&self) -> Vec<(Key, SchemaValue)> {
        match self {
            SchemaValue::List(items) => items.clone(),
            _ => Vec::new(),
        }
    }

    fn first(&self) -> Option<SchemaValue> {
        match self {
            SchemaValue::List(items) => items
                .iter()
                .find(|(k, _)| *k == Key::Int(0))
                .map(|(_, v)| v.clone()),
            _ => None,
        }
    }

    fn property(&self, key: &Key) -> Option<SchemaValue> {
        let name = key_name(key);
        match self {
            SchemaValue::Object(props) => props
                .iter()
                .find(|(k, v)| *k == name && *v != SchemaValue::Null)
                .map(|(_, v)| v.clone()),
            _ => None,
        }
    }

    fn null() -> SchemaValue {
        SchemaValue::Null
    }

    fn array(entries: Vec<(Key, SchemaValue)>) -> SchemaValue {
        SchemaValue::List(entries)
    }
}
