//! The allowlist globals (`$allowedposttags`, `$allowedtags`, `$allowedentitynames`, `$allowedxmlentitynames`).

use std::rc::Rc;

use speedyspec_hooks::{Key, Value};
use speedyspec_wp_kses_contracts::EntityNames;
use speedyspec_wp_kses_lexical::{
    default_html_entity_name_list, default_html_entity_names, default_xml_entity_names,
};

use crate::{default_post_value, default_tags_value};

/// Where kses reads its allowlist globals. Plugins write to these in PHP, so the PHP adapter reads them live; Rust
/// callers use [`DefaultGlobals`].
pub trait AllowedGlobals {
    fn post(&self) -> Value;
    fn tags(&self) -> Value;
    fn entity_names(&self) -> Rc<EntityNames>;
    /// `$allowedentitynames` as the PHP value it is (`wp_kses_allowed_html( 'entities' )` returns it).
    fn entity_names_value(&self) -> Value;
    fn xml_entity_names(&self) -> Rc<EntityNames>;
    /// Whether `CUSTOM_TAGS` is defined true.
    fn custom_tags(&self) -> bool;
    /// Changes whenever a global is written, so converted allowlists can be cached between writes.
    fn generation(&self) -> u64;
}

/// The pinned WordPress release's defaults, never written.
pub struct DefaultGlobals {
    entity_names: Rc<EntityNames>,
    xml_entity_names: Rc<EntityNames>,
}

impl Default for DefaultGlobals {
    fn default() -> Self {
        DefaultGlobals {
            entity_names: Rc::new(default_html_entity_names().clone()),
            xml_entity_names: Rc::new(default_xml_entity_names().clone()),
        }
    }
}

impl AllowedGlobals for DefaultGlobals {
    fn post(&self) -> Value {
        default_post_value()
    }
    fn tags(&self) -> Value {
        default_tags_value()
    }
    fn entity_names(&self) -> Rc<EntityNames> {
        self.entity_names.clone()
    }
    fn xml_entity_names(&self) -> Rc<EntityNames> {
        self.xml_entity_names.clone()
    }
    fn entity_names_value(&self) -> Value {
        Value::Array(
            default_html_entity_name_list()
                .iter()
                .enumerate()
                .map(|(i, n)| {
                    (
                        Key::Int(i as i64),
                        Value::Str(String::from_utf8_lossy(n).into_owned()),
                    )
                })
                .collect(),
        )
    }
    fn custom_tags(&self) -> bool {
        false
    }
    fn generation(&self) -> u64 {
        0
    }
}
