//! Test doubles for the API's ports, and WordPress's shortcode test handlers in Rust.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use speedyspec_wp_shortcode::{HandAttsParser, HandScanner, MemoryRegistry, NoFilters, ShortcodeApi, ShortcodeDeps};
use speedyspec_wp_shortcode_contracts::{
    AttKey, DiagnosticMessage, Diagnostics, HtmlSplitter, KsesAttributes, ParsedAtts, PhpKey, PhpStrings, PhpValue,
    ShortcodeError, ShortcodeFilters, ShortcodeHandler, php_bytes,
};

use super::api_cases::{recorded_attr_parse, recorded_html_split, recorded_one_attr};

type HandlerFn = dyn Fn(&ParsedAtts, Option<&[u8]>, &[u8]) -> Result<PhpValue, ShortcodeError>;

/// A handler made from a closure.
pub struct FnHandler(pub Box<HandlerFn>);

impl ShortcodeHandler for FnHandler {
    fn call(&self, atts: &ParsedAtts, content: Option<&[u8]>, tag: &[u8]) -> Result<PhpValue, ShortcodeError> {
        (self.0)(atts, content, tag)
    }
}

/// A handler whose callback is no longer callable.
pub struct Uncallable;

impl ShortcodeHandler for Uncallable {
    fn call(&self, _: &ParsedAtts, _: Option<&[u8]>, _: &[u8]) -> Result<PhpValue, ShortcodeError> {
        unreachable!("an uncallable handler is never called")
    }

    fn is_callable(&self) -> bool {
        false
    }
}

pub fn handler(f: impl Fn(&ParsedAtts, Option<&[u8]>, &[u8]) -> Result<PhpValue, ShortcodeError> + 'static) -> Rc<dyn ShortcodeHandler> {
    Rc::new(FnHandler(Box::new(f)))
}

/// PHP's string conversion for the scalar values tests use.
pub struct SimpleStrings;

impl PhpStrings for SimpleStrings {
    fn to_php_string(&self, value: &PhpValue) -> Result<Vec<u8>, ShortcodeError> {
        Ok(match value {
            PhpValue::Null | PhpValue::Bool(false) => Vec::new(),
            PhpValue::Bool(true) => b"1".to_vec(),
            PhpValue::Int(i) => i.to_string().into_bytes(),
            PhpValue::Float(f) => f.to_string().into_bytes(),
            PhpValue::Str(s) => s.clone().into_bytes(),
            PhpValue::Bytes(b) => b.clone(),
            PhpValue::Array(_) => b"Array".to_vec(),
            PhpValue::Opaque(_) => return Err(ShortcodeError::Conversion("object".into())),
        })
    }
}

/// Records notices as `doing_it_wrong:function|message|version` and `deprecated_null_subject:function`.
#[derive(Default)]
pub struct RecordingDiagnostics(pub RefCell<Vec<Vec<u8>>>);

fn sprintf(template: &str, args: &[Vec<u8>]) -> Vec<u8> {
    let mut out = template.as_bytes().to_vec();
    for (i, arg) in args.iter().enumerate() {
        let positional = format!("%{}$s", i + 1);
        out = replace(&out, positional.as_bytes(), arg);
    }
    if let Some(arg) = args.first() {
        out = replace(&out, b"%s", arg);
    }
    out
}

fn replace(hay: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < hay.len() {
        if hay[i..].starts_with(from) {
            out.extend_from_slice(to);
            i += from.len();
        } else {
            out.push(hay[i]);
            i += 1;
        }
    }
    out
}

impl Diagnostics for RecordingDiagnostics {
    fn doing_it_wrong(&self, function: &str, message: DiagnosticMessage, version: &str) -> Result<(), ShortcodeError> {
        let mut line = format!("doing_it_wrong:{function}|").into_bytes();
        line.extend(sprintf(message.template, &message.args));
        line.extend(format!("|{version}").into_bytes());
        self.0.borrow_mut().push(line);
        Ok(())
    }

    fn deprecated_null_subject(&self, function: &str) -> Result<(), ShortcodeError> {
        self.0.borrow_mut().push(format!("deprecated_null_subject:{function}").into_bytes());
        Ok(())
    }

    fn undefined_array_key(&self, key: &PhpKey) -> Result<(), ShortcodeError> {
        let key = match key {
            PhpKey::Int(i) => i.to_string(),
            PhpKey::Str(s) => format!("{s:?}"),
        };
        self.0.borrow_mut().push(format!("warning:Undefined array key {key}").into_bytes());
        Ok(())
    }
}

/// kses, answering from calls recorded against WordPress.
pub struct ScriptedKses {
    attr: HashMap<Vec<u8>, Option<Vec<Vec<u8>>>>,
    one: HashMap<(Vec<u8>, Vec<u8>), Vec<u8>>,
}

impl ScriptedKses {
    pub fn recorded() -> Self {
        Self {
            attr: recorded_attr_parse()
                .into_iter()
                .map(|(e, r)| (e.to_vec(), r.map(|v| v.into_iter().map(<[u8]>::to_vec).collect())))
                .collect(),
            one: recorded_one_attr().into_iter().map(|(a, e, r)| ((a.to_vec(), e.to_vec()), r.to_vec())).collect(),
        }
    }
}

impl KsesAttributes for ScriptedKses {
    fn attr_parse<'a>(&self, element: &'a [u8]) -> Option<Vec<&'a [u8]>> {
        let parts = self
            .attr
            .get(element)
            .unwrap_or_else(|| panic!("no recorded wp_kses_attr_parse for {:?}", String::from_utf8_lossy(element)))
            .as_ref()?;
        // Re-slice from `element`, so the parts borrow from it as the real parser's do.
        let mut at = 0;
        Some(
            parts
                .iter()
                .map(|p| {
                    let slice = &element[at..at + p.len()];
                    assert_eq!(slice, p.as_slice(), "kses parts are consecutive slices");
                    at += p.len();
                    slice
                })
                .collect(),
        )
    }

    fn one_attr(&self, attr: &[u8], element: &[u8]) -> Vec<u8> {
        self.one
            .get(&(attr.to_vec(), element.to_vec()))
            .unwrap_or_else(|| panic!("no recorded wp_kses_one_attr for {:?}", String::from_utf8_lossy(attr)))
            .clone()
    }
}

/// `wp_html_split()`, answering from calls recorded against WordPress.
pub struct ScriptedSplit(HashMap<Vec<u8>, Vec<Vec<u8>>>);

impl ScriptedSplit {
    pub fn recorded() -> Self {
        Self(
            recorded_html_split()
                .into_iter()
                .map(|(c, parts)| (c.to_vec(), parts.into_iter().map(<[u8]>::to_vec).collect()))
                .collect(),
        )
    }
}

impl HtmlSplitter for ScriptedSplit {
    fn split<'a>(&self, content: &'a [u8]) -> Vec<&'a [u8]> {
        // Content with no '<' is one text chunk; anything else must have been recorded.
        if !content.contains(&b'<') {
            return vec![content];
        }
        let parts = self
            .0
            .get(content)
            .unwrap_or_else(|| panic!("no recorded wp_html_split for {:?}", String::from_utf8_lossy(content)));
        let mut at = 0;
        parts
            .iter()
            .map(|p| {
                let slice = &content[at..at + p.len()];
                at += p.len();
                slice
            })
            .collect()
    }
}

/// The ports a test API is built from, so tests can inspect the doubles.
pub struct Harness {
    pub api: Rc<ShortcodeApi>,
    pub registry: Rc<MemoryRegistry>,
    pub diagnostics: Rc<RecordingDiagnostics>,
}

/// An API over the doubles, with `filters` as given.
pub fn harness_with(filters: Rc<dyn ShortcodeFilters>) -> Harness {
    let registry = Rc::new(MemoryRegistry::new());
    let diagnostics = Rc::new(RecordingDiagnostics::default());
    let api = Rc::new(ShortcodeApi::new(ShortcodeDeps {
        registry: registry.clone(),
        scanner: Rc::new(HandScanner::new()),
        atts: Rc::new(HandAttsParser::new()),
        filters,
        diagnostics: diagnostics.clone(),
        html: Rc::new(ScriptedSplit::recorded()),
        kses: Rc::new(ScriptedKses::recorded()),
        strings: Rc::new(SimpleStrings),
    }));
    Harness { api, registry, diagnostics }
}

pub fn harness() -> Harness {
    harness_with(Rc::new(NoFilters))
}

fn ser_atts(atts: &ParsedAtts) -> Vec<u8> {
    let mut out = Vec::new();
    for (k, v) in atts.iter() {
        match k {
            AttKey::Int(i) => out.extend(format!("i{i}=").into_bytes()),
            AttKey::Str(s) => {
                out.push(b's');
                out.extend_from_slice(s);
                out.push(b'=');
            }
        }
        out.extend_from_slice(v);
        out.push(b';');
    }
    out
}

fn ser_content(content: Option<&[u8]>) -> Vec<u8> {
    match content {
        None => b"NULL".to_vec(),
        Some(c) => [b"C:".as_slice(), c].concat(),
    }
}

fn key_str(k: &AttKey) -> Vec<u8> {
    match k {
        AttKey::Int(i) => i.to_string().into_bytes(),
        AttKey::Str(s) => s.clone(),
    }
}

/// Registers the core stubs and WordPress's test handlers, in WordPress's order.
pub fn register_wordpress_handlers(h: &Harness) {
    let api = &h.api;
    api.remove_all_shortcodes();
    for tag in ["wp_caption", "caption", "gallery", "playlist", "audio", "video", "embed"] {
        api.add_shortcode(
            tag.as_bytes(),
            handler(|atts, content, tag| {
                Ok(php_bytes([b"X{".as_slice(), tag, b"|", &ser_atts(atts), b"|", &ser_content(content), b"}"].concat()))
            }),
        );
    }
    api.add_shortcode(
        b"test-shortcode-tag",
        handler(|atts, content, tag| {
            Ok(php_bytes([b"T{".as_slice(), &ser_atts(atts), b"|", &ser_content(content), b"|", tag, b"}"].concat()))
        }),
    );
    api.add_shortcode(
        b"footag",
        handler(|atts, _, _| {
            let value = atts.iter().find(|(k, _)| **k == AttKey::Str(b"foo".to_vec())).map(|(_, v)| v).unwrap_or(b"");
            Ok(php_bytes([b"foo = ".as_slice(), value].concat()))
        }),
    );
    let weak: Weak<ShortcodeApi> = Rc::downgrade(api);
    api.add_shortcode(
        b"bartag",
        handler(move |atts, _, _| {
            let api = weak.upgrade().expect("api");
            let pairs = PhpValue::Array(vec![
                (PhpKey::Str("foo".into()), PhpValue::Str("no foo".into())),
                (PhpKey::Str("baz".into()), PhpValue::Str("default baz".into())),
            ]);
            let out = api.shortcode_atts(&pairs, &atts.to_value(), b"bartag")?;
            let PhpValue::Array(entries) = out else { panic!("shortcode_atts returns an array") };
            let value = entries.into_iter().find(|(k, _)| *k == PhpKey::Str("foo".into())).map(|(_, v)| v).unwrap();
            Ok(php_bytes([b"foo = ".as_slice(), &SimpleStrings.to_php_string(&value)?].concat()))
        }),
    );
    let weak: Weak<ShortcodeApi> = Rc::downgrade(api);
    api.add_shortcode(
        b"baztag",
        handler(move |_, content, _| {
            let api = weak.upgrade().expect("api");
            Ok(php_bytes([b"content = ".as_slice(), &api.do_shortcode(content.unwrap_or(b""), false)?].concat()))
        }),
    );
    api.add_shortcode(
        b"dumptag",
        handler(|atts, _, _| {
            let mut out = Vec::new();
            for (k, v) in atts.iter() {
                out.extend(key_str(k));
                out.extend_from_slice(b" = ");
                out.extend_from_slice(v);
                out.push(b'\n');
            }
            Ok(php_bytes(out))
        }),
    );
    for (tag, out) in [("hyphen", "shortcode_hyphen"), ("hyphen-foo", "shortcode_hyphen_foo"), ("hyphen-foo-bar", "shortcode_hyphen_foo_bar")] {
        api.add_shortcode(tag.as_bytes(), handler(move |_, _, _| Ok(PhpValue::Str(out.into()))));
    }
    api.add_shortcode(b"url", handler(|_, _, _| Ok(PhpValue::Str("http://www.wordpress.org/".into()))));
    api.add_shortcode(
        b"img",
        handler(|atts, _, _| {
            let mut out = b"<img".to_vec();
            for (k, v) in atts.iter() {
                out.push(b' ');
                out.extend(key_str(k));
                out.extend_from_slice(b"=\"");
                out.extend_from_slice(v);
                out.push(b'"');
            }
            out.extend_from_slice(b" />");
            Ok(php_bytes(out))
        }),
    );
}
