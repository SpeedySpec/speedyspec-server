//! The native functions `shortcodes.php` calls, one per WordPress function, each through a handler resolved once.

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_cqrs::{BusError, Command, HandlerRef, Query, QueryRef};
use speedyspec_hooks::{Key, Value};
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_php::native::{self, NativeError};
use speedyspec_wp_shortcode::{
    AddShortcode, DoShortcode, DoShortcodeTag, DoShortcodesInHtmlTags, GetShortcodeRegex, GetShortcodeTagsInContent,
    HasShortcode, RemoveAllShortcodes, RemoveShortcode, ShortcodeAtts, ShortcodeExists, ShortcodeParseAtts,
    StripShortcodeTag, StripShortcodes, UnescapeInvalidShortcodes,
};
use speedyspec_wp_shortcode_contracts::{PhpStrings, ShortcodeError, TagName, TagSet, php_bytes, php_str_bytes};

use crate::handler::{PhpShortcodeHandler, RustHandlers, atts_from_value};
use crate::legacy::PhpStringConversion;

const NATIVE: &str = "SpeedySpec\\WpShortcode\\Native\\";

/// The handlers every native function dispatches through, each resolved once.
pub(crate) struct Handlers {
    add: HandlerRef<AddShortcode>,
    remove: HandlerRef<RemoveShortcode>,
    remove_all: HandlerRef<RemoveAllShortcodes>,
    exists: QueryRef<ShortcodeExists>,
    has: QueryRef<HasShortcode>,
    tags_in: QueryRef<GetShortcodeTagsInContent>,
    do_shortcode: QueryRef<DoShortcode>,
    regex: QueryRef<GetShortcodeRegex>,
    tag: QueryRef<DoShortcodeTag>,
    in_html: QueryRef<DoShortcodesInHtmlTags>,
    unescape: QueryRef<UnescapeInvalidShortcodes>,
    parse: QueryRef<ShortcodeParseAtts>,
    atts: QueryRef<ShortcodeAtts>,
    strip: QueryRef<StripShortcodes>,
    strip_tag: QueryRef<StripShortcodeTag>,
}

impl Handlers {
    pub(crate) fn new(c: &Rc<Container>) -> Self {
        Self {
            add: HandlerRef::new(c.clone()),
            remove: HandlerRef::new(c.clone()),
            remove_all: HandlerRef::new(c.clone()),
            exists: QueryRef::new(c.clone()),
            has: QueryRef::new(c.clone()),
            tags_in: QueryRef::new(c.clone()),
            do_shortcode: QueryRef::new(c.clone()),
            regex: QueryRef::new(c.clone()),
            tag: QueryRef::new(c.clone()),
            in_html: QueryRef::new(c.clone()),
            unescape: QueryRef::new(c.clone()),
            parse: QueryRef::new(c.clone()),
            atts: QueryRef::new(c.clone()),
            strip: QueryRef::new(c.clone()),
            strip_tag: QueryRef::new(c.clone()),
        }
    }
}

type R = Result<Value, NativeError>;

/// A native function's body, given the handlers.
type Body = Box<dyn Fn(&Handlers, &[Value]) -> R>;

/// A shortcode error as PHP sees it: a PHP exception rethrown, PHP unwinding left alone, anything else an `Error`.
pub(crate) fn native_error(e: ShortcodeError) -> NativeError {
    match e {
        ShortcodeError::Thrown(t) => match t.payload.downcast_ref::<Value>() {
            Some(Value::Null) | None => NativeError::Silent,
            Some(exception) => NativeError::Rethrow(exception.clone()),
        },
        other => NativeError::throw("Error", other.to_string()),
    }
}

fn bus(e: BusError) -> NativeError {
    NativeError::throw("Error", e.to_string())
}

fn ask<Q: Query>(r: &QueryRef<Q>, q: Q) -> Result<Q::Result, NativeError> {
    r.call(q).map_err(bus)
}

fn send<C: Command>(r: &HandlerRef<C>, c: C) -> Result<C::Result, NativeError> {
    r.call(c).map_err(bus)
}

fn arg(args: &[Value], i: usize) -> Value {
    args.get(i).cloned().unwrap_or(Value::Null)
}

/// An argument as a PHP string, by PHP's conversion.
fn bytes(args: &[Value], i: usize) -> Result<Vec<u8>, NativeError> {
    PhpStringConversion.to_php_string(&arg(args, i)).map_err(native_error)
}

/// PHP's truthiness of an argument.
fn truthy(args: &[Value], i: usize) -> bool {
    match arg(args, i) {
        Value::Null | Value::Bool(false) | Value::Int(0) => false,
        Value::Float(f) => f != 0.0,
        Value::Str(s) => !(s.is_empty() || s == "0"),
        Value::Bytes(b) => !(b.is_empty() || b == b"0"),
        Value::Array(a) => !a.is_empty(),
        _ => true,
    }
}

/// The values of a PHP list of tag names, each by PHP's string conversion.
fn names(value: &Value) -> Result<Vec<Vec<u8>>, NativeError> {
    match value {
        Value::Array(entries) => entries
            .iter()
            .map(|(_, v)| PhpStringConversion.to_php_string(v).map_err(native_error))
            .collect(),
        _ => Ok(Vec::new()),
    }
}

fn list(items: Vec<Vec<u8>>) -> Value {
    Value::Array(items.into_iter().enumerate().map(|(i, b)| (Key::Int(i as i64), php_bytes(b))).collect())
}

/// True when WordPress returns the content as given, unconverted: its string form has no `[`.
fn no_bracket(args: &[Value]) -> Result<bool, NativeError> {
    Ok(match php_str_bytes(&arg(args, 0)) {
        Some(b) => !b.contains(&b'['),
        None => !bytes(args, 0)?.contains(&b'['),
    })
}

/// Every native function, dispatching through `h`.
pub(crate) fn functions(h: Rc<Handlers>) -> Vec<FunctionBuilder<'static>> {
    let f = |name: &str, body: Body| {
        let h = h.clone();
        native::function(&format!("{NATIVE}{name}"), move |args| body(&h, args))
    };
    vec![
        f("add_shortcode", Box::new(|h, a| {
            send(&h.add, AddShortcode { tag: bytes(a, 0)?, handler: Rc::new(PhpShortcodeHandler::new(arg(a, 1))) })?;
            Ok(Value::Null)
        })),
        f("remove_shortcode", Box::new(|h, a| {
            send(&h.remove, RemoveShortcode { tag: bytes(a, 0)? })?;
            Ok(Value::Null)
        })),
        f("remove_all_shortcodes", Box::new(|h, _| {
            send(&h.remove_all, RemoveAllShortcodes {})?;
            Ok(Value::Null)
        })),
        f("shortcode_exists", Box::new(|h, a| Ok(Value::Bool(ask(&h.exists, ShortcodeExists { tag: bytes(a, 0)? })?)))),
        f("has_shortcode", Box::new(|h, a| {
            let found = ask(&h.has, HasShortcode { content: bytes(a, 0)?, tag: bytes(a, 1)? })?;
            Ok(Value::Bool(found.map_err(native_error)?))
        })),
        f("get_shortcode_tags_in_content", Box::new(|h, a| {
            let tags = ask(&h.tags_in, GetShortcodeTagsInContent { content: bytes(a, 0)? })?;
            Ok(list(tags.map_err(native_error)?))
        })),
        f("do_shortcode", Box::new(|h, a| {
            if no_bracket(a)? {
                return Ok(arg(a, 0));
            }
            let out = ask(&h.do_shortcode, DoShortcode { content: bytes(a, 0)?, ignore_html: truthy(a, 1) })?;
            Ok(php_bytes(out.map_err(native_error)?))
        })),
        f("get_shortcode_regex", Box::new(|h, a| {
            let given = names(&arg(a, 0))?;
            Ok(php_bytes(ask(&h.regex, GetShortcodeRegex { tagnames: Some(given) })?))
        })),
        f("do_shortcode_tag", Box::new(|h, a| ask(&h.tag, DoShortcodeTag { m: arg(a, 0) })?.map_err(native_error))),
        f("do_shortcodes_in_html_tags", Box::new(|h, a| {
            let tags = TagSet::new(names(&arg(a, 2))?.into_iter().map(TagName::from_global).collect(), 0);
            let out = ask(&h.in_html, DoShortcodesInHtmlTags { content: bytes(a, 0)?, ignore_html: truthy(a, 1), tags })?;
            Ok(php_bytes(out.map_err(native_error)?))
        })),
        f("unescape_invalid_shortcodes", Box::new(|h, a| {
            Ok(php_bytes(ask(&h.unescape, UnescapeInvalidShortcodes { content: bytes(a, 0)? })?))
        })),
        f("shortcode_parse_atts", Box::new(|h, a| Ok(ask(&h.parse, ShortcodeParseAtts { text: bytes(a, 0)? })?.to_value()))),
        f("shortcode_atts", Box::new(|h, a| {
            let out = ask(&h.atts, ShortcodeAtts { pairs: arg(a, 0), atts: arg(a, 1), shortcode: bytes(a, 2)? })?;
            out.map_err(native_error)
        })),
        f("strip_shortcodes", Box::new(|h, a| {
            if no_bracket(a)? {
                return Ok(arg(a, 0));
            }
            Ok(php_bytes(ask(&h.strip, StripShortcodes { content: bytes(a, 0)? })?.map_err(native_error)?))
        })),
        f("strip_shortcode_tag", Box::new(|h, a| ask(&h.strip_tag, StripShortcodeTag { m: arg(a, 0) }))),
    ]
}

/// The native functions Rust handlers are declared as, so PHP can call them by name like any callback.
pub(crate) fn rust_handler_functions(rust: &RustHandlers) -> Vec<FunctionBuilder<'static>> {
    rust.0
        .borrow()
        .values()
        .map(|named| {
            let named = named.clone();
            native::function(&named.name.clone(), move |args| {
                let atts = atts_from_value(&arg(args, 0), &PhpStringConversion);
                let content = match arg(args, 1) {
                    Value::Null => None,
                    other => Some(PhpStringConversion.to_php_string(&other).map_err(native_error)?),
                };
                let tag = bytes(args, 2)?;
                named.inner.call(&atts, content.as_deref(), &tag).map_err(native_error)
            })
        })
        .collect()
}
