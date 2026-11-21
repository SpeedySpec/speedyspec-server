//! The native functions `kses.php` calls, one per WordPress function. Each use case of the kses API dispatches
//! through its handler, resolved once; the pure string functions of kses.php run straight from the lexical crate.

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_cqrs::{BusError, Command, HandlerRef, Query, QueryRef};
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_php::native::{self, NativeError};
use speedyspec_php_ext::{Key, Value};
use speedyspec_wp_kses::{
    AllowedFrom, AllowedGlobals, AllowedHtmlExplicitValue, AllowedHtmlValue, AllowedProtocolsQuery,
    FilterKses, FilterNohtmlKses, FilterPostKses, KsesApi, KsesAttr, KsesAttrCheck, KsesData,
    KsesHook, KsesInit, KsesInitFilters, KsesPost, KsesQuery, KsesRemoveFilters, KsesSplit,
    KsesSplit2, OneAttr, SafecssFilterAttr, UploadsLocation, UriAttributesQuery,
    add_global_attributes, allow_note_mention_span, allow_pdf_objects, attr_rule,
    attr_rules_from_value, directive_binds_value_restricted_attribute, restricts_attribute_values,
};
use speedyspec_wp_kses_contracts::{
    AllowedSpec, Context, EntityContext, KsesError, Payload, Protocols,
};
use speedyspec_wp_kses_lexical as lexical;

use crate::adapters::{PhpAllowedGlobals, PhpUploads};

const NATIVE: &str = "SpeedySpec\\WpKses\\Native\\";

/// The handlers every native function dispatches through, each resolved once.
pub(crate) struct Handlers {
    kses: QueryRef<KsesQuery>,
    post: QueryRef<KsesPost>,
    data: QueryRef<KsesData>,
    filter: QueryRef<FilterKses>,
    filter_post: QueryRef<FilterPostKses>,
    filter_nohtml: QueryRef<FilterNohtmlKses>,
    one_attr: QueryRef<OneAttr>,
    allowed_value: QueryRef<AllowedHtmlValue>,
    explicit_value: QueryRef<AllowedHtmlExplicitValue>,
    allowed_from: QueryRef<AllowedFrom>,
    uris: QueryRef<UriAttributesQuery>,
    protocols: QueryRef<AllowedProtocolsQuery>,
    safecss: QueryRef<SafecssFilterAttr>,
    hook: QueryRef<KsesHook>,
    split: QueryRef<KsesSplit>,
    split2: QueryRef<KsesSplit2>,
    attr: QueryRef<KsesAttr>,
    attr_check: QueryRef<KsesAttrCheck>,
    init_filters: HandlerRef<KsesInitFilters>,
    remove_filters: HandlerRef<KsesRemoveFilters>,
    init: HandlerRef<KsesInit>,
}

impl Handlers {
    pub(crate) fn new(c: &Rc<Container>) -> Handlers {
        Handlers {
            kses: QueryRef::new(c.clone()),
            post: QueryRef::new(c.clone()),
            data: QueryRef::new(c.clone()),
            filter: QueryRef::new(c.clone()),
            filter_post: QueryRef::new(c.clone()),
            filter_nohtml: QueryRef::new(c.clone()),
            one_attr: QueryRef::new(c.clone()),
            allowed_value: QueryRef::new(c.clone()),
            explicit_value: QueryRef::new(c.clone()),
            allowed_from: QueryRef::new(c.clone()),
            uris: QueryRef::new(c.clone()),
            protocols: QueryRef::new(c.clone()),
            safecss: QueryRef::new(c.clone()),
            hook: QueryRef::new(c.clone()),
            split: QueryRef::new(c.clone()),
            split2: QueryRef::new(c.clone()),
            attr: QueryRef::new(c.clone()),
            attr_check: QueryRef::new(c.clone()),
            init_filters: HandlerRef::new(c.clone()),
            remove_filters: HandlerRef::new(c.clone()),
            init: HandlerRef::new(c.clone()),
        }
    }
}

pub(crate) type R = Result<Value, NativeError>;

/// A native function's body.
type Native = fn(&[Value]) -> R;

// ---------------------------------------------------------------------------------------------------------------
// PHP values.

/// A PHP value as `(string)` gives it.
pub(crate) fn bytes(v: &Value) -> Vec<u8> {
    match &*v.read() {
        Value::Str(s) => s.as_bytes().to_vec(),
        Value::Bytes(b) => b.clone(),
        Value::Int(i) => i.to_string().into_bytes(),
        Value::Float(f) => php_float(*f).into_bytes(),
        Value::Bool(true) => b"1".to_vec(),
        Value::Bool(false) | Value::Null => Vec::new(),
        Value::Array(_) => b"Array".to_vec(),
        Value::Opaque(_) => Vec::new(),
    }
}

/// A float as PHP prints it (precision 17, shortest round trip; whole numbers without `.0`).
fn php_float(f: f64) -> String {
    if f.is_nan() {
        return "NAN".into();
    }
    if f.is_infinite() {
        return if f > 0.0 { "INF".into() } else { "-INF".into() };
    }
    if f == f.trunc() && f.abs() < 1e15 {
        return format!("{}", f as i64);
    }
    format!("{f}")
}

/// Bytes as a PHP string.
pub(crate) fn value(b: &[u8]) -> Value {
    match std::str::from_utf8(b) {
        Ok(s) => Value::Str(s.to_string()),
        Err(_) => Value::Bytes(b.to_vec()),
    }
}

/// PHP's truth of a value.
pub(crate) fn truthy(v: &Value) -> bool {
    match &*v.read() {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Int(i) => *i != 0,
        Value::Float(f) => *f != 0.0,
        Value::Str(s) => !(s.is_empty() || s == "0"),
        Value::Bytes(b) => !(b.is_empty() || b == b"0"),
        Value::Array(items) => !items.is_empty(),
        Value::Opaque(_) => true,
    }
}

/// `(int)` of a PHP value: a string's leading number, as PHP reads it.
fn int(v: &Value) -> i64 {
    match &*v.read() {
        Value::Int(i) => *i,
        Value::Float(f) => *f as i64,
        Value::Bool(b) => i64::from(*b),
        Value::Null | Value::Array(_) | Value::Opaque(_) => 0,
        other => {
            let b = bytes(other);
            let t = trim_start(&b);
            let (neg, digits) = match t.first() {
                Some(b'-') => (true, &t[1..]),
                Some(b'+') => (false, &t[1..]),
                _ => (false, t),
            };
            let n = digits
                .iter()
                .take_while(|c| c.is_ascii_digit())
                .fold(0i64, |n, c| {
                    n.saturating_mul(10).saturating_add(i64::from(c - b'0'))
                });
            if neg { -n } else { n }
        }
    }
}

fn trim_start(b: &[u8]) -> &[u8] {
    let n = b
        .iter()
        .take_while(|c| matches!(c, b' ' | b'\t' | b'\n' | b'\r' | b'\x0B' | b'\x0C'))
        .count();
    &b[n..]
}

/// `hexdec()`: every hex digit read, anything else skipped.
fn hexdec(v: &Value) -> u128 {
    bytes(v)
        .iter()
        .filter_map(|c| (*c as char).to_digit(16))
        .fold(0u128, |n, d| {
            n.saturating_mul(16).saturating_add(u128::from(d))
        })
}

fn a(args: &[Value], i: usize) -> Value {
    args.get(i).cloned().unwrap_or(Value::Null)
}

/// `$array[ $key ]` of a PHP array, `None` when unset.
fn index<'a>(v: &'a Value, key: &Key) -> Option<&'a Value> {
    match v {
        Value::Array(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

/// A list as PHP builds one.
fn list<'a>(items: impl IntoIterator<Item = &'a [u8]>) -> Value {
    Value::Array(
        items
            .into_iter()
            .enumerate()
            .map(|(i, b)| (Key::Int(i as i64), value(b)))
            .collect(),
    )
}

/// An array key as PHP makes one from a string: a canonical decimal integer becomes an integer key.
fn key(name: &[u8]) -> Key {
    let text = String::from_utf8_lossy(name);
    let digits = text.strip_prefix('-').unwrap_or(&text);
    let canonical = !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && !(digits.len() > 1 && digits.starts_with('0'))
        && text != "-0";
    match canonical.then(|| text.parse::<i64>().ok()).flatten() {
        Some(i) => Key::Int(i),
        None => Key::Str(text.into_owned()),
    }
}

/// `$allowed_protocols` as kses reads it: the array's values (a scalar is `(array)` cast).
fn protocols(v: &Value) -> Protocols {
    match &*v.read() {
        Value::Array(items) => Protocols(items.iter().map(|(_, p)| bytes(p).into()).collect()),
        Value::Null => Protocols(Vec::new()),
        other => Protocols(vec![bytes(other).into()]),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Errors and dispatch.

/// A kses error as PHP sees it: a PHP exception rethrown, PHP unwinding left alone, anything else an `Error`.
fn native_error(e: KsesError) -> NativeError {
    match e {
        // A payload that is not a PHP exception is a bridge's mark that PHP is unwinding (a fatal error, `exit()`).
        KsesError::Foreign { payload, .. } => match payload.0.downcast_ref::<Value>() {
            Some(exception) => NativeError::Rethrow(exception.clone()),
            None => NativeError::Silent,
        },
        other => NativeError::throw("Error", other.to_string()),
    }
}

fn bus(e: BusError) -> NativeError {
    NativeError::throw("Error", e.to_string())
}

fn ask<Q: Query<Result = Result<T, KsesError>>, T>(
    r: &QueryRef<Q>,
    q: Q,
) -> Result<T, NativeError> {
    r.call(q).map_err(bus)?.map_err(native_error)
}

fn send<C: Command<Result = Result<T, KsesError>>, T>(
    r: &HandlerRef<C>,
    c: C,
) -> Result<T, NativeError> {
    r.call(c).map_err(bus)?.map_err(native_error)
}

fn shared() -> Result<Rc<crate::Shared>, NativeError> {
    crate::SHARED
        .with(|s| s.borrow().clone())
        .ok_or_else(|| NativeError::throw("Error", "the kses API is not available"))
}

fn h<T>(f: impl FnOnce(&Handlers) -> Result<T, NativeError>) -> Result<T, NativeError> {
    f(&shared()?.handlers)
}

fn api() -> Result<Rc<KsesApi>, NativeError> {
    Ok(shared()?.api.clone())
}

fn globals() -> Result<Rc<PhpAllowedGlobals>, NativeError> {
    Ok(shared()?.globals.clone())
}

/// `$allowed_html` as kses takes it: an array as it is, anything else a context name.
fn allowed_spec(v: &Value) -> Result<AllowedSpec, NativeError> {
    let v = v.read();
    match &*v {
        // kses reads the converted array; hooks that receive `$allowed_html` get the caller's own array.
        Value::Array(_) => {
            let source = v.clone().into_owned();
            let html = h(|h| {
                ask(
                    &h.allowed_from,
                    AllowedFrom {
                        allowed: source.clone(),
                    },
                )
            })?;
            Ok(AllowedSpec::Foreign {
                html,
                source: Payload(Rc::new(source)),
            })
        }
        other => Ok(AllowedSpec::Context(Context::from_bytes(&bytes(other)))),
    }
}

fn text(r: Result<Vec<u8>, NativeError>) -> R {
    r.map(|b| value(&b))
}

// ---------------------------------------------------------------------------------------------------------------
// The functions.

fn wp_kses(args: &[Value]) -> R {
    let p = protocols(&a(args, 2));
    let m = KsesQuery {
        content: bytes(&a(args, 0)),
        allowed: allowed_spec(&a(args, 1))?,
        protocols: (!p.0.is_empty()).then_some(p),
    };
    text(h(|h| ask(&h.kses, m)))
}

fn wp_kses_one_attr(args: &[Value]) -> R {
    let m = OneAttr {
        attr: bytes(&a(args, 0)),
        element: bytes(&a(args, 1)),
    };
    text(h(|h| ask(&h.one_attr, m)))
}

fn wp_kses_allowed_html(args: &[Value]) -> R {
    let context = a(args, 0).read().into_owned();
    match context {
        Value::Array(_) => h(|h| {
            ask(
                &h.explicit_value,
                AllowedHtmlExplicitValue { allowed: context },
            )
        }),
        other => {
            let context = Context::from_bytes(&bytes(&other));
            h(|h| ask(&h.allowed_value, AllowedHtmlValue { context }))
        }
    }
}

fn allow_note_mention_span_native(args: &[Value]) -> R {
    Ok(allow_note_mention_span(
        &a(args, 0).read(),
        &a(args, 1).read(),
    ))
}

fn wp_kses_hook(args: &[Value]) -> R {
    let m = KsesHook {
        content: bytes(&a(args, 0)),
        allowed: allowed_spec(&a(args, 1))?,
        protocols: protocols(&a(args, 2)),
    };
    text(h(|h| ask(&h.hook, m)))
}

fn wp_kses_version(_: &[Value]) -> R {
    Ok(Value::Str(KsesApi::version().to_string()))
}

fn wp_kses_split(args: &[Value]) -> R {
    let m = KsesSplit {
        content: bytes(&a(args, 0)),
        allowed: allowed_spec(&a(args, 1))?,
        protocols: protocols(&a(args, 2)),
    };
    text(h(|h| ask(&h.split, m)))
}

fn wp_kses_uri_attributes(_: &[Value]) -> R {
    let uris = h(|h| ask(&h.uris, UriAttributesQuery {}))?;
    Ok(list(uris.0.iter().map(|u| &u[..])))
}

/// `_wp_kses_split_callback()`: the allowlist and protocols come from the globals `wp_kses_split()` sets.
fn wp_kses_split_callback(args: &[Value]) -> R {
    let matches = a(args, 0).read().into_owned();
    let token = index(&matches, &Key::Int(0)).map(bytes).unwrap_or_default();
    let allowed = speedyspec_php::global("pass_allowed_html").unwrap_or(Value::Null);
    let protos = speedyspec_php::global("pass_allowed_protocols").unwrap_or(Value::Null);
    let m = KsesSplit2 {
        content: token,
        allowed: allowed_spec(&allowed)?,
        protocols: protocols(&protos),
    };
    text(h(|h| ask(&h.split2, m)))
}

fn wp_kses_split2(args: &[Value]) -> R {
    let m = KsesSplit2 {
        content: bytes(&a(args, 0)),
        allowed: allowed_spec(&a(args, 1))?,
        protocols: protocols(&a(args, 2)),
    };
    text(h(|h| ask(&h.split2, m)))
}

fn wp_kses_attr(args: &[Value]) -> R {
    let m = KsesAttr {
        element: bytes(&a(args, 0)),
        attr: bytes(&a(args, 1)),
        allowed: allowed_spec(&a(args, 2))?,
        protocols: protocols(&a(args, 3)),
    };
    text(h(|h| ask(&h.attr, m)))
}

/// `wp_kses_attr_check()` as `[ok, name, value, whole]`: the facade writes the three back to its references.
fn wp_kses_attr_check(args: &[Value]) -> R {
    let rejected = || {
        Value::Array(vec![
            (Key::Int(0), Value::Bool(false)),
            (Key::Int(1), Value::Str(String::new())),
            (Key::Int(2), Value::Str(String::new())),
            (Key::Int(3), Value::Str(String::new())),
        ])
    };
    let allowed = a(args, 5).read().into_owned();
    if !matches!(allowed, Value::Array(_)) {
        return Ok(rejected());
    }
    let allowed = h(|h| ask(&h.allowed_from, AllowedFrom { allowed }))?;
    let m = KsesAttrCheck {
        name: bytes(&a(args, 0)),
        value: bytes(&a(args, 1)),
        whole: bytes(&a(args, 2)),
        vless: bytes(&a(args, 3)) == b"y",
        element: bytes(&a(args, 4)),
        allowed,
    };
    Ok(match h(|h| ask(&h.attr_check, m))? {
        None => rejected(),
        Some(checked) => Value::Array(vec![
            (Key::Int(0), Value::Bool(true)),
            (Key::Int(1), a(args, 0)),
            (Key::Int(2), value(&checked.value)),
            (Key::Int(3), value(&checked.whole)),
        ]),
    })
}

/// `wp_kses_hair()`: attributes keyed by name. A numeric name is an integer key, and so is its `name` (and its
/// `whole`, when valueless), as the tag processor's names reach PHP.
fn wp_kses_hair(args: &[Value]) -> R {
    let attr = bytes(&a(args, 0));
    let p = protocols(&a(args, 1));
    let uris = h(|h| ask(&h.uris, UriAttributesQuery {}))?;
    let attrs = lexical::hair(&attr, &p, &uris);
    Ok(Value::Array(
        attrs
            .iter()
            .map(|at| {
                let k = key(&at.name);
                let name = match &k {
                    Key::Int(i) => Value::Int(*i),
                    Key::Str(_) => value(&at.name),
                };
                let whole = if at.vless {
                    name.clone()
                } else {
                    value(&at.whole)
                };
                let entry = Value::Array(vec![
                    (Key::Str("name".into()), name),
                    (Key::Str("value".into()), value(&at.value)),
                    (Key::Str("whole".into()), whole),
                    (
                        Key::Str("vless".into()),
                        Value::Str(if at.vless { "y" } else { "n" }.into()),
                    ),
                ]);
                (k, entry)
            })
            .collect(),
    ))
}

fn parts(found: Option<Vec<&[u8]>>) -> Value {
    match found {
        None => Value::Bool(false),
        Some(parts) => list(parts),
    }
}

fn wp_kses_attr_parse(args: &[Value]) -> R {
    Ok(parts(lexical::attr_parse(&bytes(&a(args, 0)))))
}

fn wp_kses_hair_parse(args: &[Value]) -> R {
    Ok(parts(lexical::hair_parse(&bytes(&a(args, 0)))))
}

fn wp_kses_check_attr_val(args: &[Value]) -> R {
    let api = api()?;
    let resolve = api.resolver();
    let rule = attr_rule(&bytes(&a(args, 2)), &a(args, 3).read(), &resolve);
    let vless = bytes(&a(args, 1)) == b"y";
    lexical::check_attr_val(&bytes(&a(args, 0)), vless, &rule)
        .map(Value::Bool)
        .map_err(native_error)
}

fn wp_kses_bad_protocol(args: &[Value]) -> R {
    Ok(value(&lexical::bad_protocol(
        &bytes(&a(args, 0)),
        &protocols(&a(args, 1)),
    )))
}

/// `wp_kses_no_null()`: `\0` runs are kept only when `$options['slash_zero']` is set to something other than
/// `'remove'`.
fn wp_kses_no_null(args: &[Value]) -> R {
    let options = a(args, 1).read().into_owned();
    let keep = match index(&options, &Key::Str("slash_zero".into())) {
        None | Some(Value::Null) => false,
        Some(Value::Str(s)) => s != "remove",
        Some(_) => true,
    };
    Ok(value(&lexical::no_null(&bytes(&a(args, 0)), keep)))
}

fn wp_kses_stripslashes(args: &[Value]) -> R {
    Ok(value(&lexical::stripslashes(&bytes(&a(args, 0)))))
}

/// `wp_kses_array_lc()` on any PHP value, as its two `(array)` casts and `strtolower()`s read it.
fn wp_kses_array_lc(args: &[Value]) -> R {
    fn as_array(v: &Value) -> Vec<(Key, Value)> {
        match v {
            Value::Array(items) => items.clone(),
            Value::Null => Vec::new(),
            other => vec![(Key::Int(0), other.clone())],
        }
    }
    fn lower(k: &Key) -> Key {
        match k {
            Key::Int(i) => Key::Int(*i),
            Key::Str(s) => key(s.to_ascii_lowercase().as_bytes()),
        }
    }
    fn put(items: &mut Vec<(Key, Value)>, k: Key, v: Value) {
        match items.iter_mut().find(|(have, _)| *have == k) {
            Some(slot) => slot.1 = v,
            None => items.push((k, v)),
        }
    }
    let mut out: Vec<(Key, Value)> = Vec::new();
    for (k, inner) in as_array(&a(args, 0).read()) {
        let outer = lower(&k);
        put(&mut out, outer.clone(), Value::Array(Vec::new()));
        for (k2, v2) in as_array(&inner.read()) {
            if let Some((_, Value::Array(slot))) = out.iter_mut().find(|(have, _)| *have == outer) {
                put(slot, lower(&k2), v2);
            }
        }
    }
    Ok(Value::Array(out))
}

fn wp_kses_html_error(args: &[Value]) -> R {
    Ok(value(lexical::html_error(&bytes(&a(args, 0)))))
}

fn wp_kses_bad_protocol_once(args: &[Value]) -> R {
    let count = args.get(2).map(int).unwrap_or(1);
    let count = u32::try_from(count).unwrap_or(0);
    Ok(value(&lexical::bad_protocol_once(
        &bytes(&a(args, 0)),
        &protocols(&a(args, 1)),
        count,
    )))
}

fn wp_kses_bad_protocol_once2(args: &[Value]) -> R {
    Ok(value(&lexical::bad_protocol_once2(
        &bytes(&a(args, 0)),
        &protocols(&a(args, 1)),
    )))
}

fn wp_kses_normalize_entities(args: &[Value]) -> R {
    let g = globals()?;
    let ctx = if bytes(&a(args, 1)) == b"xml" {
        EntityContext::Xml
    } else {
        EntityContext::Html
    };
    let content = bytes(&a(args, 0));
    let out = lexical::normalize_entities(&content, ctx, &g.entity_names(), &g.xml_entity_names());
    Ok(value(&out))
}

/// `$matches[1]` of a preg callback, and whether it is a non-empty string (`in_array( ..., true )` matches only
/// strings).
fn group(args: &[Value]) -> (Value, bool) {
    let matches = a(args, 0).read().into_owned();
    let m = index(&matches, &Key::Int(1))
        .cloned()
        .unwrap_or(Value::Null);
    let is_string = matches!(m, Value::Str(_) | Value::Bytes(_));
    (m, is_string)
}

fn wp_kses_named_entities(args: &[Value]) -> R {
    let (m, is_string) = group(args);
    if !truthy(&m) {
        return Ok(Value::Str(String::new()));
    }
    let i = bytes(&m);
    let allowed = is_string && globals()?.entity_names().0.contains(&i[..]);
    Ok(value(&if allowed {
        [&b"&"[..], &i, b";"].concat()
    } else {
        [&b"&amp;"[..], &i, b";"].concat()
    }))
}

fn wp_kses_xml_named_entities(args: &[Value]) -> R {
    let (m, is_string) = group(args);
    if !truthy(&m) {
        return Ok(Value::Str(String::new()));
    }
    let i = bytes(&m);
    let g = globals()?;
    let reference = [&b"&"[..], &i, b";"].concat();
    Ok(value(
        &if is_string && g.xml_entity_names().0.contains(&i[..]) {
            reference
        } else if is_string && g.entity_names().0.contains(&i[..]) {
            lexical::html_entity_decode(&reference)
        } else {
            [&b"&amp;"[..], &i, b";"].concat()
        },
    ))
}

fn valid(i: i64) -> bool {
    u64::try_from(i).is_ok_and(lexical::valid_unicode)
}

fn wp_kses_normalize_entities2(args: &[Value]) -> R {
    let (m, _) = group(args);
    if !truthy(&m) {
        return Ok(Value::Str(String::new()));
    }
    let i = bytes(&m);
    Ok(value(&if valid(int(&m)) {
        let trimmed: Vec<u8> = i.iter().copied().skip_while(|c| *c == b'0').collect();
        let padded = if trimmed.len() < 3 {
            [vec![b'0'; 3 - trimmed.len()], trimmed].concat()
        } else {
            trimmed
        };
        [&b"&#"[..], &padded, b";"].concat()
    } else {
        [&b"&amp;#"[..], &i, b";"].concat()
    }))
}

fn wp_kses_normalize_entities3(args: &[Value]) -> R {
    let (m, _) = group(args);
    if !truthy(&m) {
        return Ok(Value::Str(String::new()));
    }
    let hex = bytes(&m);
    let code = hexdec(&m);
    let ok = u64::try_from(code).is_ok_and(lexical::valid_unicode);
    Ok(value(&if ok {
        let trimmed: Vec<u8> = hex.iter().copied().skip_while(|c| *c == b'0').collect();
        [&b"&#x"[..], &trimmed, b";"].concat()
    } else {
        [&b"&amp;#x"[..], &hex, b";"].concat()
    }))
}

fn valid_unicode(args: &[Value]) -> R {
    Ok(Value::Bool(valid(int(&a(args, 0)))))
}

fn wp_kses_decode_entities(args: &[Value]) -> R {
    Ok(value(&lexical::decode_entities(&bytes(&a(args, 0)))))
}

/// `chr()`: the code point taken modulo 256.
fn chr(n: i128) -> Value {
    value(&[n.rem_euclid(256) as u8])
}

fn decode_entities_chr(args: &[Value]) -> R {
    let (m, _) = group(args);
    Ok(chr(i128::from(int(&m))))
}

fn decode_entities_chr_hexdec(args: &[Value]) -> R {
    let (m, _) = group(args);
    Ok(chr((hexdec(&m) % 256) as i128))
}

fn wp_filter_kses(args: &[Value]) -> R {
    let data = bytes(&a(args, 0));
    text(h(|h| ask(&h.filter, FilterKses { data })))
}

fn wp_kses_data(args: &[Value]) -> R {
    let data = bytes(&a(args, 0));
    text(h(|h| ask(&h.data, KsesData { data })))
}

fn wp_filter_post_kses(args: &[Value]) -> R {
    let data = bytes(&a(args, 0));
    text(h(|h| ask(&h.filter_post, FilterPostKses { data })))
}

fn wp_kses_post(args: &[Value]) -> R {
    let data = bytes(&a(args, 0));
    text(h(|h| ask(&h.post, KsesPost { data })))
}

fn wp_filter_nohtml_kses(args: &[Value]) -> R {
    let data = bytes(&a(args, 0));
    text(h(|h| ask(&h.filter_nohtml, FilterNohtmlKses { data })))
}

fn kses_init_filters(_: &[Value]) -> R {
    h(|h| send(&h.init_filters, KsesInitFilters {}))?;
    Ok(Value::Null)
}

fn kses_remove_filters(_: &[Value]) -> R {
    h(|h| send(&h.remove_filters, KsesRemoveFilters {}))?;
    Ok(Value::Null)
}

fn kses_init(_: &[Value]) -> R {
    h(|h| send(&h.init, KsesInit {}))?;
    Ok(Value::Null)
}

fn safecss_filter_attr(args: &[Value]) -> R {
    let css = bytes(&a(args, 0));
    text(h(|h| ask(&h.safecss, SafecssFilterAttr { css })))
}

fn wp_add_global_attributes(args: &[Value]) -> R {
    Ok(add_global_attributes(&a(args, 0).read()))
}

fn wp_kses_allow_pdf_objects(args: &[Value]) -> R {
    let url = bytes(&a(args, 0));
    let uploads = PhpUploads.upload_url().map_err(native_error)?;
    Ok(Value::Bool(allow_pdf_objects(&url, &uploads)))
}

fn restricts_attribute_values_native(args: &[Value]) -> R {
    let attrs = a(args, 0).read().into_owned();
    if !matches!(attrs, Value::Array(_)) {
        return Ok(Value::Bool(false));
    }
    let api = api()?;
    let resolve = api.resolver();
    Ok(Value::Bool(restricts_attribute_values(
        &attr_rules_from_value(&attrs, &resolve),
    )))
}

fn directive_binds_native(args: &[Value]) -> R {
    let directive = bytes(&a(args, 0));
    let attrs = a(args, 1).read().into_owned();
    let api = api()?;
    let resolve = api.resolver();
    let rules = attr_rules_from_value(&attrs, &resolve);
    Ok(Value::Bool(directive_binds_value_restricted_attribute(
        &directive, &rules,
    )))
}

fn wp_allowed_protocols(_: &[Value]) -> R {
    let protocols = h(|h| ask(&h.protocols, AllowedProtocolsQuery {}))?;
    Ok(list(protocols.0.iter().map(|p| &p[..])))
}

/// Every native function, by the WordPress function it serves.
pub(crate) fn entries() -> Vec<(&'static str, Native)> {
    vec![
        ("wp_kses", wp_kses),
        ("wp_kses_one_attr", wp_kses_one_attr),
        ("wp_kses_allowed_html", wp_kses_allowed_html),
        (
            "_wp_kses_allow_note_mention_span",
            allow_note_mention_span_native,
        ),
        ("wp_kses_hook", wp_kses_hook),
        ("wp_kses_version", wp_kses_version),
        ("wp_kses_split", wp_kses_split),
        ("wp_kses_uri_attributes", wp_kses_uri_attributes),
        ("_wp_kses_split_callback", wp_kses_split_callback),
        ("wp_kses_split2", wp_kses_split2),
        ("wp_kses_attr", wp_kses_attr),
        ("wp_kses_attr_check", wp_kses_attr_check),
        ("wp_kses_hair", wp_kses_hair),
        ("wp_kses_attr_parse", wp_kses_attr_parse),
        ("wp_kses_hair_parse", wp_kses_hair_parse),
        ("wp_kses_check_attr_val", wp_kses_check_attr_val),
        ("wp_kses_bad_protocol", wp_kses_bad_protocol),
        ("wp_kses_no_null", wp_kses_no_null),
        ("wp_kses_stripslashes", wp_kses_stripslashes),
        ("wp_kses_array_lc", wp_kses_array_lc),
        ("wp_kses_html_error", wp_kses_html_error),
        ("wp_kses_bad_protocol_once", wp_kses_bad_protocol_once),
        ("wp_kses_bad_protocol_once2", wp_kses_bad_protocol_once2),
        ("wp_kses_normalize_entities", wp_kses_normalize_entities),
        ("wp_kses_named_entities", wp_kses_named_entities),
        ("wp_kses_xml_named_entities", wp_kses_xml_named_entities),
        ("wp_kses_normalize_entities2", wp_kses_normalize_entities2),
        ("wp_kses_normalize_entities3", wp_kses_normalize_entities3),
        ("valid_unicode", valid_unicode),
        ("wp_kses_decode_entities", wp_kses_decode_entities),
        ("_wp_kses_decode_entities_chr", decode_entities_chr),
        (
            "_wp_kses_decode_entities_chr_hexdec",
            decode_entities_chr_hexdec,
        ),
        ("wp_filter_kses", wp_filter_kses),
        ("wp_kses_data", wp_kses_data),
        ("wp_filter_post_kses", wp_filter_post_kses),
        ("wp_kses_post", wp_kses_post),
        ("wp_filter_nohtml_kses", wp_filter_nohtml_kses),
        ("kses_init_filters", kses_init_filters),
        ("kses_remove_filters", kses_remove_filters),
        ("kses_init", kses_init),
        ("safecss_filter_attr", safecss_filter_attr),
        ("_wp_add_global_attributes", wp_add_global_attributes),
        ("_wp_kses_allow_pdf_objects", wp_kses_allow_pdf_objects),
        (
            "_wp_kses_restricts_attribute_values",
            restricts_attribute_values_native,
        ),
        (
            "_wp_kses_directive_binds_value_restricted_attribute",
            directive_binds_native,
        ),
        ("wp_allowed_protocols", wp_allowed_protocols),
    ]
}

pub(crate) fn functions() -> Vec<FunctionBuilder<'static>> {
    entries()
        .into_iter()
        .map(|(name, f)| native::function(&format!("{NATIVE}{name}"), f))
        .collect()
}
