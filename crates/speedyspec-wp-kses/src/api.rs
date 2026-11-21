//! [`KsesApi`], the facade over kses's functions, and the filters it fires.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use speedyspec_hooks::{Key, Value};
use speedyspec_wp_hooks::{Count, HookApi, HookError, HookName};
use speedyspec_wp_kses_contracts::{
    AllowedHtml, AttributeEscaper, Bytes, Context, KsesError, Protocols, UnfilteredHtmlPolicy,
    UriAttributes, ValueCallback,
};

use crate::{
    AllowedGlobals, UploadsLocation, ValueCallbacks, allow_pdf_objects, allowed_from_value,
};

/// Converted allowlists by context, with the globals' generation they were converted at.
type AllowedCache = RefCell<HashMap<Vec<u8>, (u64, Rc<AllowedHtml>)>>;

/// What kses needs from outside: the Hook API and the ports to code not yet ported.
pub struct KsesDeps {
    pub hooks: Rc<HookApi>,
    pub escaper: Rc<dyn AttributeEscaper>,
    pub policy: Rc<dyn UnfilteredHtmlPolicy>,
    pub globals: Rc<dyn AllowedGlobals>,
    pub uploads: Rc<dyn UploadsLocation>,
    pub callbacks: Rc<dyn ValueCallbacks>,
}

/// WordPress's kses functions over the Rust Hook API (Facade). One per request, as WordPress's `static` caches are.
pub struct KsesApi {
    pub(crate) d: KsesDeps,
    allowed_cache: AllowedCache,
    default_uris: Rc<UriAttributes>,
    protocols: RefCell<Vec<Value>>,
}

pub(crate) fn hook_error(e: HookError) -> KsesError {
    KsesError::Hook(e.message)
}

/// The 17 attributes `wp_kses_uri_attributes()` starts from.
const URI_ATTRIBUTES: &[&str] = &[
    "action",
    "archive",
    "background",
    "cite",
    "classid",
    "codebase",
    "data",
    "formaction",
    "href",
    "icon",
    "longdesc",
    "manifest",
    "poster",
    "profile",
    "src",
    "usemap",
    "xmlns",
];

/// `wp_allowed_protocols()`'s defaults.
const PROTOCOLS: &[&str] = &[
    "http", "https", "ftp", "ftps", "mailto", "news", "irc", "irc6", "ircs", "gopher", "nntp",
    "feed", "telnet", "mms", "rtsp", "sms", "svn", "tel", "fax", "xmpp", "webcal", "urn",
];

fn list(items: &[&str]) -> Value {
    Value::Array(
        items
            .iter()
            .enumerate()
            .map(|(i, s)| (Key::Int(i as i64), Value::Str((*s).to_string())))
            .collect(),
    )
}

/// A PHP value as `(string)` gives it, for lists of names.
pub(crate) fn php_string(v: &Value) -> Option<Vec<u8>> {
    match v {
        Value::Str(s) => Some(s.as_bytes().to_vec()),
        Value::Bytes(b) => Some(b.clone()),
        Value::Int(i) => Some(i.to_string().into_bytes()),
        Value::Bool(true) => Some(b"1".to_vec()),
        Value::Bool(false) | Value::Null => Some(Vec::new()),
        Value::Float(f) => Some(f.to_string().into_bytes()),
        _ => None,
    }
}

pub(crate) fn bytes_value(b: &[u8]) -> Value {
    match std::str::from_utf8(b) {
        Ok(s) => Value::Str(s.to_string()),
        Err(_) => Value::Bytes(b.to_vec()),
    }
}

/// `isset( $array[ $key ] )`.
fn isset(v: &Value, key: &str) -> bool {
    matches!(v, Value::Array(items) if items.iter().any(|(k, v)| matches!(k, Key::Str(s) if s == key) && !matches!(v, Value::Null)))
}

/// `$array[ $key ] = $value`, keeping the key's place when it exists.
fn set(v: &mut Value, key: &str, value: Value) {
    if !matches!(v, Value::Array(_)) {
        *v = Value::Array(Vec::new());
    }
    if let Value::Array(items) = v {
        match items
            .iter_mut()
            .find(|(k, _)| matches!(k, Key::Str(s) if s == key))
        {
            Some(slot) => slot.1 = value,
            None => items.push((Key::Str(key.to_string()), value)),
        }
    }
}

fn entry<'a>(v: &'a mut Value, key: &str) -> &'a mut Value {
    if !isset(v, key) {
        set(v, key, Value::Array(Vec::new()));
    }
    let Value::Array(items) = v else {
        unreachable!("set made an array")
    };
    &mut items
        .iter_mut()
        .find(|(k, _)| matches!(k, Key::Str(s) if s == key))
        .expect("set inserted the key")
        .1
}

impl KsesApi {
    pub fn new(deps: KsesDeps) -> KsesApi {
        KsesApi {
            d: deps,
            allowed_cache: RefCell::new(HashMap::new()),
            default_uris: Rc::new(UriAttributes(
                URI_ATTRIBUTES.iter().map(|u| u.as_bytes().into()).collect(),
            )),
            protocols: RefCell::new(Vec::new()),
        }
    }

    pub fn deps(&self) -> &KsesDeps {
        &self.d
    }

    /// Runs `apply_filters()`, or counts the run and returns `None` when nothing listens.
    pub(crate) fn filter(
        &self,
        hook: &str,
        value: impl FnOnce() -> Value,
        args: impl FnOnce() -> Vec<Value>,
    ) -> Result<Option<Value>, KsesError> {
        let name = HookName::from(hook);
        if self
            .d
            .hooks
            .skip_if_empty(&name, Count::Filters)
            .map_err(hook_error)?
        {
            return Ok(None);
        }
        self.d
            .hooks
            .apply_filters(&name, value(), args())
            .map(Some)
            .map_err(hook_error)
    }

    /// Turns a `value_callback` into a callback: `_wp_kses_allow_pdf_objects` natively, anything else through the
    /// [`ValueCallbacks`] port.
    pub(crate) fn resolver(&self) -> impl Fn(&Value) -> Option<ValueCallback> + '_ {
        move |v: &Value| match v {
            Value::Str(s) if s == "_wp_kses_allow_pdf_objects" => {
                let uploads = self.d.uploads.clone();
                let callback: ValueCallback =
                    Rc::new(move |url: &[u8]| Ok(allow_pdf_objects(url, &uploads.upload_url()?)));
                Some(callback)
            }
            other => self.d.callbacks.resolve(other),
        }
    }

    fn convert(&self, v: &Value) -> Result<Rc<AllowedHtml>, KsesError> {
        Ok(Rc::new(allowed_from_value(v, &self.resolver())?))
    }

    /// One `apply_filters( 'wp_kses_allowed_html', $base, $context )`: with no listener, the base converted once
    /// per write of the globals.
    fn filtered_allowed(
        &self,
        ctx: &[u8],
        base: &Value,
    ) -> Result<(Rc<AllowedHtml>, Value), KsesError> {
        let ctx_value = bytes_value(ctx);
        match self.filter("wp_kses_allowed_html", || base.clone(), || vec![ctx_value])? {
            Some(filtered) => Ok((self.convert(&filtered)?, filtered)),
            None => {
                let generation = self.d.globals.generation();
                let cached = self
                    .allowed_cache
                    .borrow()
                    .get(ctx)
                    .filter(|(g, _)| *g == generation)
                    .map(|(_, c)| c.clone());
                if let Some(cached) = cached {
                    return Ok((cached, base.clone()));
                }
                let converted = self.convert(base)?;
                self.allowed_cache
                    .borrow_mut()
                    .insert(ctx.to_vec(), (generation, converted.clone()));
                Ok((converted, base.clone()))
            }
        }
    }

    /// `wp_kses_allowed_html()` for a named context.
    pub fn allowed_html(&self, ctx: &Context) -> Result<Rc<AllowedHtml>, KsesError> {
        let name = ctx.as_bytes();
        let globals = &self.d.globals;
        match ctx {
            Context::Post => {
                let post = globals.post();
                let (tags, value) = self.filtered_allowed(name, &post)?;
                if !globals.custom_tags()
                    && !isset(&value, "form")
                    && (isset(&value, "input") || isset(&value, "select"))
                {
                    let mut with_form = post;
                    let form: Vec<(Key, Value)> = [
                        "action",
                        "accept",
                        "accept-charset",
                        "enctype",
                        "method",
                        "name",
                        "target",
                    ]
                    .iter()
                    .map(|a| (Key::Str((*a).to_string()), Value::Bool(true)))
                    .collect();
                    set(&mut with_form, "form", Value::Array(form));
                    let ctx_value = bytes_value(name);
                    let filtered = self.filter(
                        "wp_kses_allowed_html",
                        || with_form.clone(),
                        || vec![ctx_value],
                    )?;
                    return self.convert(&filtered.unwrap_or(with_form));
                }
                Ok(tags)
            }
            Context::UserDescription
            | Context::PreTermDescription
            | Context::PreUserDescription => {
                let mut tags = globals.tags();
                let a = entry(&mut tags, "a");
                set(a, "rel", Value::Bool(true));
                set(a, "target", Value::Bool(true));
                Ok(self.filtered_allowed(name, &tags)?.0)
            }
            Context::Strip => Ok(self.filtered_allowed(name, &Value::Array(Vec::new()))?.0),
            Context::Entities => {
                let names = globals.entity_names();
                let mut sorted: Vec<&Bytes> = names.0.iter().collect();
                sorted.sort();
                let value = Value::Array(
                    sorted
                        .iter()
                        .enumerate()
                        .map(|(i, n)| (Key::Int(i as i64), bytes_value(n)))
                        .collect(),
                );
                Ok(self.filtered_allowed(name, &value)?.0)
            }
            Context::Data | Context::Named(_) => {
                Ok(self.filtered_allowed(name, &globals.tags())?.0)
            }
        }
    }

    /// `wp_kses_allowed_html()` given an array: filtered with the context `explicit`.
    pub fn allowed_html_explicit(&self, allowed: &Value) -> Result<Rc<AllowedHtml>, KsesError> {
        let filtered = self.filter(
            "wp_kses_allowed_html",
            || allowed.clone(),
            || vec![Value::Str("explicit".into())],
        )?;
        self.convert(filtered.as_ref().unwrap_or(allowed))
    }

    /// `wp_kses_uri_attributes()`.
    pub fn uri_attributes(&self) -> Result<Rc<UriAttributes>, KsesError> {
        match self.filter("wp_kses_uri_attributes", || list(URI_ATTRIBUTES), Vec::new)? {
            None => Ok(self.default_uris.clone()),
            Some(Value::Array(items)) => Ok(Rc::new(UriAttributes(
                items
                    .iter()
                    .filter_map(|(_, v)| php_string(v).map(Into::into))
                    .collect(),
            ))),
            Some(_) => Ok(Rc::new(UriAttributes(Vec::new()))),
        }
    }

    /// `wp_allowed_protocols()`, with its `static` list: refiltered on every call until `wp_loaded` has run.
    pub fn allowed_protocols(&self) -> Result<Rc<Protocols>, KsesError> {
        let mut protocols = self.protocols.borrow_mut();
        if protocols.is_empty() {
            *protocols = match list(PROTOCOLS) {
                Value::Array(items) => items.into_iter().map(|(_, v)| v).collect(),
                _ => Vec::new(),
            };
        }
        let loaded = self
            .d
            .hooks
            .did_action(&HookName::from("wp_loaded"))
            .map_err(hook_error)?
            > 0;
        if !loaded {
            let current = Value::Array(
                protocols
                    .iter()
                    .cloned()
                    .enumerate()
                    .map(|(i, v)| (Key::Int(i as i64), v))
                    .collect(),
            );
            if let Some(filtered) = self.filter("kses_allowed_protocols", || current, Vec::new)? {
                let items: Vec<Value> = match filtered {
                    Value::Array(items) => items.into_iter().map(|(_, v)| v).collect(),
                    Value::Null => Vec::new(),
                    scalar => vec![scalar],
                };
                let mut unique: Vec<Value> = Vec::new();
                for item in items {
                    if !unique.iter().any(|u| php_string(u) == php_string(&item)) {
                        unique.push(item);
                    }
                }
                *protocols = unique;
            }
        }
        Ok(Rc::new(Protocols(
            protocols
                .iter()
                .filter_map(|v| php_string(v).map(Into::into))
                .collect(),
        )))
    }
}
