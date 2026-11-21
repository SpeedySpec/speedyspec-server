use std::collections::HashSet;
use std::rc::Rc;

use speedyspec_wp_shortcode_contracts::{
    AttKey, AttsParser, DiagnosticMessage, Diagnostics, HtmlSplitter, InvalidTagName, KsesAttributes, MatchGroups, ParsedAtts,
    PhpKey, PhpStrings, PhpValue, ShortcodeError, ShortcodeFilters, ShortcodeHandler, ShortcodeRegistry,
    ShortcodeScanner, TagName, TagSet, get_shortcode_regex, php_bytes, php_str_bytes,
};

use crate::scan::candidate_tokens;
use crate::strtr::unescape;

/// Everything [`ShortcodeApi`] works through: the registry, the two strategies and the ports.
pub struct ShortcodeDeps {
    pub registry: Rc<dyn ShortcodeRegistry>,
    pub scanner: Rc<dyn ShortcodeScanner>,
    pub atts: Rc<dyn AttsParser>,
    pub filters: Rc<dyn ShortcodeFilters>,
    pub diagnostics: Rc<dyn Diagnostics>,
    pub html: Rc<dyn HtmlSplitter>,
    pub kses: Rc<dyn KsesAttributes>,
    pub strings: Rc<dyn PhpStrings>,
}

/// WordPress's Shortcode API: the Facade, one method per function in `shortcodes.php`.
///
/// No method holds a borrow while a handler or filter runs, so handlers may call back into it.
pub struct ShortcodeApi {
    d: ShortcodeDeps,
}

/// Which callback `preg_replace_callback()` runs for each match.
#[derive(Clone, Copy)]
pub(crate) enum Callback {
    Do,
    Strip,
}

/// PHP's `empty()` for a string: `""` and `"0"`.
fn php_empty(s: &[u8]) -> bool {
    s.is_empty() || s == b"0"
}

/// `substr( $s, 1, -1 )`.
fn trim_one(s: &[u8]) -> Vec<u8> {
    if s.len() < 2 { Vec::new() } else { s[1..s.len() - 1].to_vec() }
}

impl ShortcodeApi {
    /// An API over `deps`.
    pub fn new(deps: ShortcodeDeps) -> Self {
        Self { d: deps }
    }

    pub(crate) fn deps(&self) -> &ShortcodeDeps {
        &self.d
    }

    /// `add_shortcode( $tag, $callback )`.
    pub fn add_shortcode(&self, tag: &[u8], handler: Rc<dyn ShortcodeHandler>) {
        match TagName::new(tag) {
            Ok(name) => self.d.registry.add(name, handler),
            Err(InvalidTagName::Empty) => self.d.diagnostics.doing_it_wrong(
                "add_shortcode",
                DiagnosticMessage { template: "Invalid shortcode name: Empty name given.", args: vec![] },
                "4.4.0",
            ),
            Err(InvalidTagName::Reserved) => self.d.diagnostics.doing_it_wrong(
                "add_shortcode",
                DiagnosticMessage {
                    template: "Invalid shortcode name: %1$s. Do not use spaces or reserved characters: %2$s",
                    args: vec![tag.to_vec(), b"& / < > [ ] =".to_vec()],
                },
                "4.4.0",
            ),
        }
    }

    /// `remove_shortcode( $tag )`.
    pub fn remove_shortcode(&self, tag: &[u8]) {
        self.d.registry.remove(tag);
    }

    /// `remove_all_shortcodes()`.
    pub fn remove_all_shortcodes(&self) {
        self.d.registry.remove_all();
    }

    /// `shortcode_exists( $tag )`.
    pub fn shortcode_exists(&self, tag: &[u8]) -> bool {
        self.d.registry.exists(tag)
    }

    fn registry_set(&self) -> TagSet {
        TagSet::new(self.d.registry.tag_names(), self.d.registry.generation())
    }

    /// `has_shortcode( $content, $tag )`.
    ///
    /// # Errors
    ///
    /// A scanner failure.
    pub fn has_shortcode(&self, content: &[u8], tag: &[u8]) -> Result<bool, ShortcodeError> {
        if memchr::memchr(b'[', content).is_none() || !self.shortcode_exists(tag) {
            return Ok(false);
        }
        let set = self.registry_set();
        let matches: Vec<MatchGroups> =
            self.d.scanner.scan(content, &set).map(|m| m.map(|m| m.groups_in(content))).collect::<Result<_, _>>()?;
        for g in matches {
            if g.0[2] == tag || (!php_empty(&g.0[5]) && self.has_shortcode(&g.0[5], tag)?) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// `get_shortcode_tags_in_content( $content )`.
    ///
    /// # Errors
    ///
    /// A scanner failure.
    pub fn get_shortcode_tags_in_content(&self, content: &[u8]) -> Result<Vec<Vec<u8>>, ShortcodeError> {
        if memchr::memchr(b'[', content).is_none() {
            return Ok(Vec::new());
        }
        let set = self.registry_set();
        let matches: Vec<MatchGroups> =
            self.d.scanner.scan(content, &set).map(|m| m.map(|m| m.groups_in(content))).collect::<Result<_, _>>()?;
        let mut tags = Vec::new();
        for g in matches {
            tags.push(g.0[2].clone());
            if !php_empty(&g.0[5]) {
                tags.extend(self.get_shortcode_tags_in_content(&g.0[5])?);
            }
        }
        Ok(tags)
    }

    /// `apply_shortcodes( $content, $ignore_html )`.
    ///
    /// # Errors
    ///
    /// As [`ShortcodeApi::do_shortcode`].
    pub fn apply_shortcodes(&self, content: &[u8], ignore_html: bool) -> Result<Vec<u8>, ShortcodeError> {
        self.do_shortcode(content, ignore_html)
    }

    /// The registered names that appear whole as a candidate token, in `names`' order.
    fn narrowed(&self, names: Vec<TagName>, content: &[u8]) -> TagSet {
        let tokens: HashSet<&[u8]> = candidate_tokens(content).collect();
        TagSet::new(
            names.into_iter().filter(|n| tokens.contains(n.as_bytes())).collect(),
            self.d.registry.generation(),
        )
    }

    /// `do_shortcode( $content, $ignore_html )`.
    ///
    /// # Errors
    ///
    /// Any error from a handler, a filter or the scanner. The context filter then stays attached, as WordPress
    /// leaves it when an exception escapes.
    pub fn do_shortcode(&self, content: &[u8], ignore_html: bool) -> Result<Vec<u8>, ShortcodeError> {
        if memchr::memchr(b'[', content).is_none() {
            return Ok(content.to_vec());
        }
        let names = self.d.registry.tag_names();
        if names.is_empty() {
            return Ok(content.to_vec());
        }
        let set = self.narrowed(names, content);
        if set.names().is_empty() {
            return Ok(content.to_vec());
        }
        let guard = self.d.filters.enter_do_shortcode_context()?;
        let content = self.do_shortcodes_in_html_tags(content, ignore_html, &set)?;
        let (content, _) = self.replace(&content, &set, Callback::Do)?;
        let content = unescape(&content);
        self.d.filters.leave_do_shortcode_context(guard)?;
        Ok(content)
    }

    /// `preg_replace_callback( "/$pattern/", $callback, $haystack, -1, $count )`.
    pub(crate) fn replace(
        &self,
        haystack: &[u8],
        tags: &TagSet,
        callback: Callback,
    ) -> Result<(Vec<u8>, usize), ShortcodeError> {
        let mut out = Vec::with_capacity(haystack.len());
        let mut last = 0;
        let mut count = 0;
        for m in self.d.scanner.scan(haystack, tags) {
            let m = m?;
            out.extend_from_slice(&haystack[last..m.start]);
            let groups = m.groups_in(haystack).to_value();
            let result = match callback {
                Callback::Do => self.do_shortcode_tag(&groups)?,
                Callback::Strip => self.strip_shortcode_tag(&groups),
            };
            out.extend(self.d.strings.to_php_string(&result)?);
            last = m.end;
            count += 1;
        }
        out.extend_from_slice(&haystack[last..]);
        Ok((out, count))
    }

    /// `$m[ $i ] ?? null` as bytes: `None` when absent or `null`, with no notice.
    fn group(&self, m: &PhpValue, i: i64) -> Result<Option<Vec<u8>>, ShortcodeError> {
        let PhpValue::Array(entries) = m else {
            return Ok(None);
        };
        match entries.iter().find(|(k, _)| *k == PhpKey::Int(i)).map(|(_, v)| v) {
            None | Some(PhpValue::Null) => Ok(None),
            Some(v) => match php_str_bytes(v) {
                Some(b) => Ok(Some(b.to_vec())),
                None => self.d.strings.to_php_string(v).map(Some),
            },
        }
    }

    /// `$m[ $i ]` as WordPress reads it, without `??`: a missing key raises PHP's warning and reads as `null`.
    fn read_group(&self, m: &PhpValue, i: i64) -> Result<Option<Vec<u8>>, ShortcodeError> {
        let present = matches!(m, PhpValue::Array(entries) if entries.iter().any(|(k, _)| *k == PhpKey::Int(i)));
        if !present {
            self.d.diagnostics.undefined_array_key(&PhpKey::Int(i));
        }
        self.group(m, i)
    }

    /// `'[' === $m[1] && ']' === $m[6]`, reading `$m[6]` only when `$m[1]` matched, as PHP short-circuits.
    fn escaped(&self, m: &PhpValue) -> Result<bool, ShortcodeError> {
        Ok(self.read_group(m, 1)?.as_deref() == Some(b"[") && self.read_group(m, 6)?.as_deref() == Some(b"]"))
    }

    /// `do_shortcode_tag( $m )`. The result is the PHP value WordPress returns, unconverted.
    ///
    /// # Errors
    ///
    /// Any error from the handler, a filter or a string conversion.
    pub fn do_shortcode_tag(&self, m: &PhpValue) -> Result<PhpValue, ShortcodeError> {
        if self.escaped(m)? {
            return Ok(php_bytes(trim_one(&self.read_group(m, 0)?.unwrap_or_default())));
        }
        let tag = self.read_group(m, 2)?.unwrap_or_default();
        let atts = self.shortcode_parse_atts(&self.read_group(m, 3)?.unwrap_or_default());
        let handler = self.d.registry.handler(&tag);
        if handler.is_none() {
            // `$shortcode_tags[ $tag ]` on a tag that is gone: PHP's key conversion applies to the notice too.
            self.d.diagnostics.undefined_array_key(&match AttKey::from_php(tag.clone()) {
                AttKey::Int(i) => PhpKey::Int(i),
                AttKey::Str(s) => PhpKey::Str(String::from_utf8_lossy(&s).into_owned()),
            });
        }
        let Some(handler) = handler.filter(|h| h.is_callable()) else {
            self.d.diagnostics.doing_it_wrong(
                "do_shortcode_tag",
                DiagnosticMessage {
                    template: "Attempting to parse a shortcode without a valid callback: %s",
                    args: vec![tag],
                },
                "4.3.0",
            );
            return Ok(match m {
                PhpValue::Array(entries) => {
                    entries.iter().find(|(k, _)| *k == PhpKey::Int(0)).map(|(_, v)| v.clone()).unwrap_or(PhpValue::Null)
                }
                _ => PhpValue::Null,
            });
        };
        let attr = atts.to_value();
        let early = self.d.filters.pre_do_shortcode_tag(&tag, &attr, m)?;
        if *early.read() != PhpValue::Bool(false) {
            return Ok(early);
        }
        let content = self.group(m, 5)?;
        let g1 = self.read_group(m, 1)?;
        let returned = handler.call(&atts, content.as_deref(), &tag)?;
        let mut output = g1.unwrap_or_default();
        output.extend(self.d.strings.to_php_string(&returned)?);
        output.extend(self.read_group(m, 6)?.unwrap_or_default());
        self.d.filters.do_shortcode_tag(php_bytes(output), &tag, &attr, m)
    }

    /// `strip_shortcode_tag( $m )`.
    pub fn strip_shortcode_tag(&self, m: &PhpValue) -> PhpValue {
        let g = |i| self.read_group(m, i).ok().flatten().unwrap_or_default();
        if self.escaped(m).unwrap_or(false) {
            return php_bytes(trim_one(&g(0)));
        }
        php_bytes([g(1), g(6)].concat())
    }

    /// `unescape_invalid_shortcodes( $content )`.
    pub fn unescape_invalid_shortcodes(&self, content: &[u8]) -> Vec<u8> {
        unescape(content)
    }

    /// `shortcode_parse_atts( $text )`, raising PHP's deprecation when WordPress does (invalid UTF-8).
    pub fn shortcode_parse_atts(&self, text: &[u8]) -> ParsedAtts {
        let parsed = self.d.atts.parse(text);
        if parsed.null_subject {
            self.d.diagnostics.deprecated_null_subject("preg_match_all");
        }
        parsed.atts
    }

    /// `shortcode_atts( $pairs, $atts, $shortcode )`.
    ///
    /// # Errors
    ///
    /// A filter failure.
    pub fn shortcode_atts(&self, pairs: &PhpValue, atts: &PhpValue, shortcode: &[u8]) -> Result<PhpValue, ShortcodeError> {
        let atts = match atts {
            PhpValue::Array(entries) => entries.clone(),
            PhpValue::Null => Vec::new(),
            other => vec![(PhpKey::Int(0), other.clone())],
        };
        let pairs_entries: &[(PhpKey, PhpValue)] = match pairs {
            PhpValue::Array(entries) => entries,
            _ => &[],
        };
        let out = PhpValue::Array(
            pairs_entries
                .iter()
                .map(|(name, default)| {
                    let value = atts.iter().find(|(k, _)| k == name).map_or_else(|| default.clone(), |(_, v)| v.clone());
                    (name.clone(), value)
                })
                .collect(),
        );
        if php_empty(shortcode) {
            return Ok(out);
        }
        self.d.filters.shortcode_atts(shortcode, out, pairs, &PhpValue::Array(atts))
    }

    /// `strip_shortcodes( $content )`.
    ///
    /// # Errors
    ///
    /// A filter, scanner or string-conversion failure.
    pub fn strip_shortcodes(&self, content: &[u8]) -> Result<Vec<u8>, ShortcodeError> {
        if memchr::memchr(b'[', content).is_none() {
            return Ok(content.to_vec());
        }
        let names = self.d.registry.tag_names();
        if names.is_empty() {
            return Ok(content.to_vec());
        }
        let listed = PhpValue::Array(
            names
                .iter()
                .enumerate()
                .map(|(i, n)| (PhpKey::Int(i as i64), php_bytes(n.as_bytes().to_vec())))
                .collect(),
        );
        let filtered = self.d.filters.strip_shortcodes_tagnames(listed, content)?;
        let mut to_remove = Vec::new();
        // A PHP filter's result may arrive unread; look inside it as PHP would.
        if let PhpValue::Array(entries) = &*filtered.read() {
            for (_, v) in entries {
                to_remove.push(TagName::from_global(match php_str_bytes(v) {
                    Some(b) => b.to_vec(),
                    None => self.d.strings.to_php_string(v)?,
                }));
            }
        }
        let set = self.narrowed(to_remove, content);
        if set.names().is_empty() {
            return Ok(content.to_vec());
        }
        let content = self.do_shortcodes_in_html_tags(content, true, &set)?;
        let (content, _) = self.replace(&content, &set, Callback::Strip)?;
        Ok(unescape(&content))
    }

    /// `get_shortcode_regex( $tagnames )`: the registered names when `tagnames` is `None` or empty.
    pub fn get_shortcode_regex(&self, tagnames: Option<&[Vec<u8>]>) -> Vec<u8> {
        match tagnames {
            Some(names) if !names.is_empty() => {
                get_shortcode_regex(&names.iter().map(|n| TagName::from_global(n.clone())).collect::<Vec<_>>())
            }
            _ => get_shortcode_regex(&self.d.registry.tag_names()),
        }
    }
}
