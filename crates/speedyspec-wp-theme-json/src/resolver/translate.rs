//! `WP_Theme_JSON_Resolver::translate()`: settings and titles translated by WordPress's `theme-i18n.json` schema,
//! through the i18n port's `translate_settings` (which also renumbers what the schema marks as lists).

use std::rc::Rc;

use speedyspec_hooks::{Key, Value};
use speedyspec_wp_i18n::{I18nError, SettingsTree, translate_settings};
use speedyspec_wp_theme_json_contracts::ports::Gettext;
use speedyspec_wp_theme_json_contracts::schema::THEME_I18N;

use crate::php::to_php_string;
use crate::php_json::{JsonTree, decode_tree};
use crate::php_value::empty;

/// A schema node (a decoded JSON object stays an object) or a settings value.
#[derive(Clone, Debug)]
enum Tree {
    Value(Value),
    Object(Rc<Vec<(String, Tree)>>),
    List(Rc<Vec<Tree>>),
}

fn from_json(tree: JsonTree) -> Tree {
    match tree {
        JsonTree::Value(v) => Tree::Value(v),
        JsonTree::Object(props) => Tree::Object(Rc::new(
            props.into_iter().map(|(k, v)| (k, from_json(v))).collect(),
        )),
        JsonTree::List(items) => Tree::List(Rc::new(items.into_iter().map(from_json).collect())),
    }
}

impl SettingsTree for Tree {
    fn is_empty(&self) -> bool {
        match self {
            Tree::Value(v) => empty(v),
            Tree::Object(_) => false,
            Tree::List(items) => items.is_empty(),
        }
    }
    fn as_str(&self) -> Option<&[u8]> {
        match self {
            Tree::Value(Value::Str(s)) => Some(s.as_bytes()),
            Tree::Value(Value::Bytes(b)) => Some(b),
            _ => None,
        }
    }
    fn is_array(&self) -> bool {
        matches!(self, Tree::Value(Value::Array(_)) | Tree::List(_))
    }
    fn is_object(&self) -> bool {
        matches!(self, Tree::Object(_))
    }
    fn entries(&self) -> Vec<(Key, Self)> {
        match self {
            Tree::Value(Value::Array(e)) => e
                .iter()
                .map(|(k, v)| (k.clone(), Tree::Value(v.clone())))
                .collect(),
            Tree::List(items) => items
                .iter()
                .enumerate()
                .map(|(i, v)| (Key::Int(i as i64), v.clone()))
                .collect(),
            _ => Vec::new(),
        }
    }
    fn first(&self) -> Option<Self> {
        match self {
            Tree::List(items) => items.first().cloned(),
            Tree::Value(Value::Array(e)) => e
                .iter()
                .find(|(k, _)| *k == Key::Int(0))
                .map(|(_, v)| Tree::Value(v.clone())),
            _ => None,
        }
    }
    fn property(&self, key: &Key) -> Option<Self> {
        let Tree::Object(props) = self else {
            return None;
        };
        let name = match key {
            Key::Int(i) => i.to_string(),
            Key::Str(s) => s.clone(),
        };
        props
            .iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| v.clone())
            .filter(|v| !matches!(v, Tree::Value(Value::Null)))
    }
    fn null() -> Self {
        Tree::Value(Value::Null)
    }
    fn array(entries: Vec<(Key, Self)>) -> Self {
        Tree::Value(Value::Array(
            entries
                .into_iter()
                .map(|(k, v)| (k, into_value(v)))
                .collect(),
        ))
    }
}

fn into_value(tree: Tree) -> Value {
    match tree {
        Tree::Value(v) => v,
        Tree::Object(props) => Value::Array(
            props
                .iter()
                .map(|(k, v)| (crate::array_path::php_key(k), into_value(v.clone())))
                .collect(),
        ),
        Tree::List(items) => Value::Array(
            items
                .iter()
                .enumerate()
                .map(|(i, v)| (Key::Int(i as i64), into_value(v.clone())))
                .collect(),
        ),
    }
}

thread_local! {
    static SCHEMA: Tree = decode_tree(THEME_I18N.as_bytes()).map(from_json).unwrap_or(Tree::Value(Value::Array(Vec::new())));
}

/// `translate( $theme_json, $domain )`.
pub fn translate(theme_json: Value, domain: &Value, gettext: &dyn Gettext) -> Value {
    SCHEMA.with(|schema| {
        let mut call = |text: &[u8], context: &[u8], domain: &Tree| -> Result<Tree, I18nError> {
            let domain = match domain {
                Tree::Value(v) => to_php_string(v),
                _ => Vec::new(),
            };
            Ok(Tree::Value(crate::codec::string_value(
                &gettext.translate_with_context(text, context, &domain),
            )))
        };
        match translate_settings(
            schema,
            Tree::Value(theme_json.clone()),
            &Tree::Value(domain.clone()),
            &mut call,
        ) {
            Ok(tree) => into_value(tree),
            Err(_) => theme_json,
        }
    })
}
