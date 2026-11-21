//! The corpus and the stand-in ports the benches share.
#![allow(dead_code)]

use std::rc::Rc;

use criterion::Criterion;
use speedyspec_wp_shortcode::{MemoryRegistry, NoFilters, ShortcodeApi, ShortcodeDeps, candidate_tokens};
use speedyspec_wp_shortcode_contracts::{
    AttsParser, DiagnosticMessage, Diagnostics, HtmlSplitter, KsesAttributes, ParsedAtts, PhpKey, PhpStrings, PhpValue,
    ShortcodeError, ShortcodeHandler, ShortcodeRegistry, ShortcodeScanner, TagName, TagSet, php_str_bytes,
};

/// The registered tags: WordPress's core ones and common plugin and page-builder ones.
pub const TAGS: &[&str] = &[
    "wp_caption", "caption", "gallery", "playlist", "audio", "video", "embed", "button", "contact-form-7", "tabs", "tab",
    "vc_row", "vc_column", "vc_row_inner", "vc_column_inner", "vc_column_text", "url", "escaped",
];

/// `(name, content)` for each corpus file, in order.
pub fn corpus() -> Vec<(String, Vec<u8>)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("benches/corpus");
    let mut files: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
    files.sort();
    files
        .into_iter()
        .map(|p| (p.file_stem().unwrap().to_string_lossy().into_owned(), std::fs::read(&p).unwrap()))
        .collect()
}

/// The tag set `do_shortcode()` would scan `content` with: the registered names that appear as whole tokens.
pub fn narrowed(content: &[u8]) -> TagSet {
    let tokens: std::collections::HashSet<&[u8]> = candidate_tokens(content).collect();
    TagSet::new(
        TAGS.iter().filter(|t| tokens.contains(t.as_bytes())).map(|t| TagName::new(t.as_bytes()).unwrap()).collect(),
        1,
    )
}

pub fn criterion() -> Criterion {
    Criterion::default()
        .sample_size(20)
        .warm_up_time(std::time::Duration::from_millis(500))
        .measurement_time(std::time::Duration::from_secs(2))
}

struct Quiet;
impl Diagnostics for Quiet {
    fn doing_it_wrong(&self, _: &str, _: DiagnosticMessage, _: &str) -> Result<(), ShortcodeError> {
        Ok(())
    }
    fn deprecated_null_subject(&self, _: &str) -> Result<(), ShortcodeError> {
        Ok(())
    }
    fn undefined_array_key(&self, _: &PhpKey) -> Result<(), ShortcodeError> {
        Ok(())
    }
}

/// `wp_html_split()`: the Rust port the PHP bridge binds.
struct Split;
impl HtmlSplitter for Split {
    fn split<'a>(&self, content: &'a [u8]) -> Vec<&'a [u8]> {
        speedyspec_wp_html_split::wp_html_split(content)
    }
}

/// kses that never parses (every element takes WordPress's fallback path).
struct NoKses;
impl KsesAttributes for NoKses {
    fn attr_parse<'a>(&self, _: &'a [u8]) -> Option<Vec<&'a [u8]>> {
        None
    }
    fn one_attr(&self, attr: &[u8], _: &[u8]) -> Vec<u8> {
        attr.to_vec()
    }
}

struct Strings;
impl PhpStrings for Strings {
    fn to_php_string(&self, value: &PhpValue) -> Result<Vec<u8>, ShortcodeError> {
        Ok(php_str_bytes(value).map(<[u8]>::to_vec).unwrap_or_default())
    }
}

/// A handler that echoes its attribute count and enclosed content, so nested shortcodes are processed too.
struct Echo(std::rc::Weak<ShortcodeApi>);
impl ShortcodeHandler for Echo {
    fn call(&self, atts: &ParsedAtts, content: Option<&[u8]>, _: &[u8]) -> Result<PhpValue, ShortcodeError> {
        let inner = match (content, self.0.upgrade()) {
            (Some(c), Some(api)) if !c.is_empty() => api.do_shortcode(c, false)?,
            _ => Vec::new(),
        };
        let mut out = format!("<span data-atts=\"{}\">", atts.iter().count()).into_bytes();
        out.extend(inner);
        out.extend_from_slice(b"</span>");
        Ok(PhpValue::Bytes(out))
    }
}

/// The API with the given strategies, the tags registered with [`Echo`].
pub fn api(scanner: Rc<dyn ShortcodeScanner>, atts: Rc<dyn AttsParser>) -> Rc<ShortcodeApi> {
    let registry = Rc::new(MemoryRegistry::new());
    let api = Rc::new(ShortcodeApi::new(ShortcodeDeps {
        registry: registry.clone(),
        scanner,
        atts,
        filters: Rc::new(NoFilters),
        diagnostics: Rc::new(Quiet),
        html: Rc::new(Split),
        kses: Rc::new(NoKses),
        strings: Rc::new(Strings),
    }));
    for tag in TAGS {
        registry.add(TagName::new(tag.as_bytes()).unwrap(), Rc::new(Echo(Rc::downgrade(&api))));
    }
    api
}
