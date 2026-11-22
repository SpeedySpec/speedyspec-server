//! The fused scanner: `wp_kses()` in one pass over the bytes per call, with `wp_kses_split2()`, `wp_kses_attr()`
//! and `wp_kses_attr_check()` handled in place as each token is met.

use std::borrow::Cow;
use std::rc::Rc;

use speedyspec_hooks::Value;
use speedyspec_wp_html_syntax::decode_attribute;
use speedyspec_wp_kses_contracts::{
    AllowedAttrs, AllowedHtml, AllowedSpec, AttrRule, AttrRules, ByteMap, EntityContext, KsesError,
    Protocols,
};
use speedyspec_wp_kses_lexical::scan::{
    TokenKind, invalid_wrapper, next_token, tag_head, xhtml_slash,
};
use speedyspec_wp_kses_lexical::{
    HairAttr, check_attr_val, hair_attrs_into, is_uri_attribute, no_null, normalize_entities,
    stripslashes, write_whole,
};

use crate::KsesApi;
use crate::api::{bytes_value, is_default_uri_attribute, php_string};
use crate::convert::allowed_to_value;

/// The most `wp_kses()` passes a comment or invalid-markup token gets to settle. Every corpus input settles within
/// 2; a token still changing after this many is one PHP never finishes, and the port drops it.
const MAX_PASSES: usize = 16;

/// The bytes the nested passes of one `wp_kses()` call may process in all, per byte of its input, plus a floor:
/// comments can nest as deep as the square root of their length (`<<!--!--` leaves `<!--` when stripped), which in
/// PHP costs time that grows faster than the input. Normal content uses a few times its length; a token past the
/// budget is dropped (compat/kses/breaks.json, technical).
const NESTED_WORK_PER_BYTE: usize = 32;
const NESTED_WORK_FLOOR: usize = 64 * 1024;

/// An attribute that passed `wp_kses_attr_check()`: its value and whole text, as the check left them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckedAttr {
    pub value: Vec<u8>,
    pub whole: Vec<u8>,
}

/// What `wp_kses_attr_check()` makes of an attribute.
enum Verdict {
    /// PHP returns false.
    Rejected,
    /// Kept as it was written.
    AsWritten,
    /// Kept with its `style` value filtered and re-encoded to this; its whole has the old value replaced by it.
    Restyled(Vec<u8>),
}

/// Moves a buffer of attributes to another lifetime, emptied: its allocation is kept (an in-place collect), so
/// `attr_into` can reuse it across tags whose attributes borrow different tokens.
fn recycle<'b>(mut v: Vec<HairAttr<'_>>) -> Vec<HairAttr<'b>> {
    v.clear();
    v.into_iter().map(|_| unreachable!("emptied")).collect()
}

/// `strtolower()`, borrowing a name that is lowercase already (most are).
fn lower(name: &[u8]) -> Cow<'_, [u8]> {
    if name.iter().any(u8::is_ascii_uppercase) {
        Cow::Owned(name.to_ascii_lowercase())
    } else {
        Cow::Borrowed(name)
    }
}

fn replace_all(s: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    if from.is_empty() {
        return s.to_vec();
    }
    let mut out = Vec::with_capacity(s.len());
    let mut at = 0;
    while let Some(i) = memchr::memmem::find(&s[at..], from) {
        out.extend_from_slice(&s[at..at + i]);
        out.extend_from_slice(to);
        at += i + from.len();
    }
    out.extend_from_slice(&s[at..]);
    out
}

/// Whether `str_replace( $value, ..., $whole )` on `whole` = `name="value"` would replace only the value in its
/// place: the value is not empty, holds no `"` (so it cannot match again past its place), and starts no match
/// earlier, inside `name="`. Then the replaced whole is `name="replacement"`, written without a search.
fn value_only_in_place(whole: &[u8], name: &[u8], value: &[u8]) -> bool {
    let in_place = name.len() + 2;
    !value.is_empty()
        && memchr::memchr(b'"', value).is_none()
        && !(0..in_place).any(|p| whole.get(p..p + value.len()) == Some(value))
}

/// Drops every `<` and `>` from `buf` past `from`, in place, a run at a time.
fn drop_angle_brackets(buf: &mut Vec<u8>, from: usize) {
    let Some(first) = memchr::memchr2(b'<', b'>', &buf[from..]) else {
        return;
    };
    let mut write = from + first;
    let mut read = write + 1;
    while read < buf.len() {
        let run = memchr::memchr2(b'<', b'>', &buf[read..]).unwrap_or(buf.len() - read);
        buf.copy_within(read..read + run, write);
        write += run;
        read += run + 1;
    }
    buf.truncate(write);
}

/// `str_replace( $needle, '', $s )` for a needle of a few bytes: found by its first byte, without building a
/// substring searcher.
fn remove_short(s: &[u8], needle: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut at = 0;
    let mut copied = 0;
    while let Some(i) = memchr::memchr(needle[0], &s[at..]) {
        let start = at + i;
        if s[start..].starts_with(needle) {
            out.extend_from_slice(&s[copied..start]);
            at = start + needle.len();
            copied = at;
        } else {
            at = start + 1;
        }
    }
    out.extend_from_slice(&s[copied..]);
    out
}

/// `/^data-[a-z0-9_-]+$/`.
fn is_data_name(name: &[u8]) -> bool {
    name.len() > 5
        && name.starts_with(b"data-")
        && name[5..]
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_' || *b == b'-')
}

/// PHP's `empty()` on an attribute's rules.
fn rules_empty(r: &AttrRules) -> bool {
    match r {
        AttrRules::Denied | AttrRules::AllowedFalsy => true,
        AttrRules::Rules(list) => list.is_empty(),
        AttrRules::Allowed => false,
    }
}

/// The lowercase key of one rule, as `array_keys()` would give it.
fn rule_key(rule: &AttrRule) -> Vec<u8> {
    match rule {
        AttrRule::MaxLen(_) => b"maxlen".to_vec(),
        AttrRule::MinLen(_) => b"minlen".to_vec(),
        AttrRule::MaxVal(_) => b"maxval".to_vec(),
        AttrRule::MinVal(_) => b"minval".to_vec(),
        AttrRule::Valueless(_) => b"valueless".to_vec(),
        AttrRule::Values(_) => b"values".to_vec(),
        AttrRule::ValueCallback(_) => b"value_callback".to_vec(),
        AttrRule::Other(k, _) => k.to_ascii_lowercase(),
    }
}

/// `_wp_kses_restricts_attribute_values()`: whether any attribute of the element has a rule beyond `required` and
/// `valueless`.
pub fn restricts_attribute_values(
    attrs: &speedyspec_wp_kses_contracts::ByteMap<AttrRules>,
) -> bool {
    attrs.values().any(|r| match r {
        AttrRules::Rules(list) => list
            .iter()
            .any(|rule| !matches!(&rule_key(rule)[..], b"required" | b"valueless")),
        _ => false,
    })
}

/// `_wp_kses_directive_binds_value_restricted_attribute()`: whether an Interactivity API `data-wp-bind--` directive
/// would replace an attribute whose values kses restricts on this element.
pub fn directive_binds_value_restricted_attribute(
    directive: &[u8],
    attrs: &speedyspec_wp_kses_contracts::ByteMap<AttrRules>,
) -> bool {
    let Some(bound) = directive.strip_prefix(b"data-wp-bind--") else {
        return false;
    };
    if !restricts_attribute_values(attrs) {
        return false;
    }
    let mut names = vec![bound];
    if let Some(i) = memchr::memmem::find(bound, b"---") {
        names.push(&bound[..i]);
    }
    for name in names {
        let rules = match attrs.get(name) {
            Some(r) => r,
            None => match attrs.get(&b"data-*"[..]) {
                Some(r) if is_data_name(name) => r,
                _ => return true,
            },
        };
        let falsy = matches!(rules, AttrRules::Denied | AttrRules::AllowedFalsy);
        if falsy || matches!(rules, AttrRules::Rules(_)) {
            return true;
        }
    }
    false
}

impl KsesApi {
    /// `wp_kses()`.
    pub fn kses(
        &self,
        content: &[u8],
        allowed: &AllowedSpec,
        protocols: Option<&Protocols>,
    ) -> Result<Vec<u8>, KsesError> {
        // The outermost call sets the budget for every nested pass beneath it.
        let outermost = self.nested_budget.get().is_none();
        if outermost {
            self.nested_budget.set(Some(
                content
                    .len()
                    .saturating_mul(NESTED_WORK_PER_BYTE)
                    .saturating_add(NESTED_WORK_FLOOR),
            ));
        }
        let result = self.kses_once(content, allowed, protocols);
        if outermost {
            self.nested_budget.set(None);
        }
        result
    }

    fn kses_once(
        &self,
        content: &[u8],
        allowed: &AllowedSpec,
        protocols: Option<&Protocols>,
    ) -> Result<Vec<u8>, KsesError> {
        let resolved;
        let protocols = match protocols {
            Some(p) if !p.0.is_empty() => p,
            _ => {
                resolved = self.allowed_protocols()?;
                &*resolved
            }
        };
        let globals = &self.d.globals;
        let content = no_null(content, true);
        let content = match normalize_entities(
            &content,
            EntityContext::Html,
            &globals.entity_names(),
            &globals.xml_entity_names(),
        ) {
            Cow::Borrowed(_) => content,
            Cow::Owned(o) => Cow::Owned(o),
        };
        let content = self.hook(content, allowed, protocols)?;
        self.split(&content, allowed, protocols)
    }

    /// `wp_kses_hook()`: the `pre_kses` filter.
    pub fn kses_hook(
        &self,
        content: &[u8],
        allowed: &AllowedSpec,
        protocols: &Protocols,
    ) -> Result<Vec<u8>, KsesError> {
        Ok(self
            .hook(Cow::Borrowed(content), allowed, protocols)?
            .into_owned())
    }

    fn hook<'a>(
        &self,
        content: Cow<'a, [u8]>,
        allowed: &AllowedSpec,
        protocols: &Protocols,
    ) -> Result<Cow<'a, [u8]>, KsesError> {
        let allowed_value = || match allowed {
            AllowedSpec::Context(c) => bytes_value(c.as_bytes()),
            AllowedSpec::Explicit(a) => allowed_to_value(a),
            AllowedSpec::Foreign { html, source } => source
                .0
                .downcast_ref::<Value>()
                .cloned()
                .unwrap_or_else(|| allowed_to_value(html)),
        };
        let protocols_value = || {
            Value::Array(
                protocols
                    .0
                    .iter()
                    .enumerate()
                    .map(|(i, p)| (speedyspec_hooks::Key::Int(i as i64), bytes_value(p)))
                    .collect(),
            )
        };
        let filtered = self.filter(
            "pre_kses",
            || bytes_value(&content),
            || vec![allowed_value(), protocols_value()],
        )?;
        Ok(match filtered {
            None => content,
            Some(v) => Cow::Owned(php_string(&v.read()).unwrap_or_default()),
        })
    }

    /// `wp_kses_split()`: every token through [`KsesApi::split2`], text between tokens copied as it is.
    pub fn split(
        &self,
        content: &[u8],
        allowed: &AllowedSpec,
        protocols: &Protocols,
    ) -> Result<Vec<u8>, KsesError> {
        let mut out = Vec::with_capacity(content.len());
        let mut at = 0;
        while let Some((start, end, kind)) = next_token(content, at) {
            out.extend_from_slice(&content[at..start]);
            self.token(&content[start..end], kind, allowed, protocols, &mut out)?;
            at = end;
        }
        out.extend_from_slice(&content[at..]);
        Ok(out)
    }

    /// `wp_kses_split2()` for one token.
    pub fn split2(
        &self,
        token: &[u8],
        allowed: &AllowedSpec,
        protocols: &Protocols,
    ) -> Result<Vec<u8>, KsesError> {
        let mut out = Vec::new();
        let kind = if token.first() == Some(&b'<') {
            TokenKind::Tag
        } else {
            TokenKind::StrayGt
        };
        self.token(token, kind, allowed, protocols, &mut out)?;
        Ok(out)
    }

    /// Repeats `wp_kses()` on `content` until it stops changing, on a stack that grows as deep input needs.
    ///
    /// @return
    ///     The settled content, or `None` when it has not settled after [`MAX_PASSES`] passes. PHP loops forever
    ///     there (`<!--</<i>` gains a `&gt;` on every pass); the caller drops the token instead.
    fn kses_until_stable(
        &self,
        mut content: Vec<u8>,
        allowed: &AllowedSpec,
        protocols: &Protocols,
    ) -> Result<Option<Vec<u8>>, KsesError> {
        for _ in 0..MAX_PASSES {
            // Each pass draws its length from the outermost call's budget; a token past it is dropped.
            match self.nested_budget.get() {
                Some(left) if left >= content.len() => {
                    self.nested_budget.set(Some(left - content.len()))
                }
                Some(_) => return Ok(None),
                None => {}
            }
            let next = stacker::maybe_grow(64 * 1024, 1024 * 1024, || {
                self.kses(&content, allowed, Some(protocols))
            })?;
            if next == content {
                return Ok(Some(content));
            }
            content = next;
        }
        Ok(None)
    }

    fn token(
        &self,
        raw: &[u8],
        _kind: TokenKind,
        allowed: &AllowedSpec,
        protocols: &Protocols,
        out: &mut Vec<u8>,
    ) -> Result<(), KsesError> {
        let token = stripslashes(raw);
        if token.first() != Some(&b'<') {
            out.extend_from_slice(b"&gt;");
            return Ok(());
        }
        if invalid_wrapper(&token) {
            let opener = token[1];
            let Some(inner) =
                self.kses_until_stable(token[2..token.len() - 1].to_vec(), allowed, protocols)?
            else {
                return Ok(());
            };
            out.push(b'<');
            out.push(opener);
            out.extend_from_slice(&inner);
            out.push(b'>');
            return Ok(());
        }
        if token.starts_with(b"<!--") {
            let inner = remove_short(&remove_short(&token, b"<!--"), b"-->");
            let Some(inner) = self.kses_until_stable(inner, allowed, protocols)? else {
                return Ok(());
            };
            if inner.is_empty() {
                return Ok(());
            }
            let mut collapsed = Vec::with_capacity(inner.len());
            for b in inner {
                if !(b == b'-' && collapsed.last() == Some(&b'-')) {
                    collapsed.push(b);
                }
            }
            if collapsed.last() == Some(&b'-') {
                collapsed.pop();
            } else if collapsed.ends_with(b"-\n") {
                let n = collapsed.len();
                collapsed.remove(n - 2);
            }
            out.extend_from_slice(b"<!--");
            out.extend_from_slice(&collapsed);
            out.extend_from_slice(b"-->");
            return Ok(());
        }
        let Some(head) = tag_head(&token) else {
            return Ok(());
        };
        let elem = &token[head.name.clone()];
        let attrlist = &token[head.attrs.clone()];
        let allowed_html = self.resolve_allowed(allowed)?;
        let Some(tag) = allowed_html.get(&lower(elem)) else {
            return Ok(());
        };
        if head.slash {
            out.extend_from_slice(b"</");
            out.extend_from_slice(elem);
            out.push(b'>');
            return Ok(());
        }
        self.attr_into(elem, Some(tag), attrlist, protocols, out)
    }

    fn resolve_allowed(&self, allowed: &AllowedSpec) -> Result<Rc<AllowedHtml>, KsesError> {
        match allowed {
            AllowedSpec::Context(c) => self.allowed_html(c),
            AllowedSpec::Explicit(a) | AllowedSpec::Foreign { html: a, .. } => Ok(a.clone()),
        }
    }

    /// `wp_kses_attr()`.
    pub fn attr(
        &self,
        element: &[u8],
        attr: &[u8],
        allowed: &AllowedSpec,
        protocols: &Protocols,
    ) -> Result<Vec<u8>, KsesError> {
        let allowed_html = self.resolve_allowed(allowed)?;
        let mut out = Vec::new();
        self.attr_into(
            element,
            allowed_html.get(&lower(element)),
            attr,
            protocols,
            &mut out,
        )?;
        Ok(out)
    }

    /// `wp_kses_attr()` for an element whose allowlist entry is `tag`, written to `out`.
    fn attr_into(
        &self,
        element: &[u8],
        tag: Option<&AllowedAttrs>,
        attr: &[u8],
        protocols: &Protocols,
        out: &mut Vec<u8>,
    ) -> Result<(), KsesError> {
        let slash: &[u8] = if xhtml_slash(attr) { b" /" } else { b"" };
        let attrs = match tag {
            Some(AllowedAttrs::Attrs(a)) if !a.is_empty() => a,
            _ => {
                out.push(b'<');
                out.extend_from_slice(element);
                out.extend_from_slice(slash);
                out.push(b'>');
                return Ok(());
            }
        };
        let uris = self.uri_attributes()?;
        let mut parsed = recycle(self.hair_scratch.take());
        let defaults = Rc::ptr_eq(&uris, &self.default_uris);
        let is_uri = |name: &[u8]| {
            if defaults {
                is_default_uri_attribute(name)
            } else {
                is_uri_attribute(&uris, name)
            }
        };
        hair_attrs_into(attr, protocols, is_uri, &mut parsed);
        let mut required: Vec<&[u8]> = attrs
            .iter()
            .filter(|(_, r)| r.is_required())
            .map(|(n, _)| &**n)
            .collect();
        let mut whole = self.scratch.take();
        // The kept attributes go straight to `out`, after the tag's name; they are taken back when the tag loses them.
        let tag_start = out.len();
        out.push(b'<');
        out.extend_from_slice(element);
        let attrs_start = out.len();
        let result = self.attrs_kept(attrs, &parsed, &mut required, out, &mut whole);
        let done = match result {
            Ok(true) => {
                drop_angle_brackets(out, attrs_start);
                out.extend_from_slice(slash);
                out.push(b'>');
                Ok(())
            }
            Ok(false) => {
                if slash.is_empty() {
                    out.truncate(attrs_start);
                    out.push(b'>');
                } else {
                    out.truncate(tag_start);
                }
                Ok(())
            }
            Err(e) => Err(e),
        };
        self.scratch.replace(whole);
        self.hair_scratch.replace(recycle(parsed));
        done
    }

    /// The attributes `attr_into` keeps, written to `kept`; false when a required attribute is missing or rejected
    /// (the tag is then stripped of every attribute).
    fn attrs_kept(
        &self,
        attrs: &ByteMap<AttrRules>,
        parsed: &[HairAttr<'_>],
        required: &mut Vec<&[u8]>,
        kept: &mut Vec<u8>,
        whole: &mut Vec<u8>,
    ) -> Result<bool, KsesError> {
        for a in parsed {
            let name_low = lower(&a.name);
            let is_required = required.iter().any(|r| **r == *name_low);
            match self.verdict(attrs, &name_low, &a.value, a.vless)? {
                Verdict::AsWritten => {
                    kept.push(b' ');
                    if a.vless {
                        kept.extend_from_slice(&a.name);
                    } else {
                        write_whole(kept, &a.name, &a.value);
                    }
                }
                Verdict::Restyled(encoded) => {
                    whole.clear();
                    if a.vless {
                        whole.extend_from_slice(&a.name);
                    } else {
                        write_whole(whole, &a.name, &a.value);
                    }
                    kept.push(b' ');
                    if !a.vless && value_only_in_place(whole, &a.name, &a.value) {
                        write_whole(kept, &a.name, &encoded);
                    } else {
                        kept.extend_from_slice(&replace_all(whole, &a.value, &encoded));
                    }
                }
                Verdict::Rejected if is_required => return Ok(false),
                Verdict::Rejected => continue,
            }
            if is_required {
                required.retain(|r| **r != *name_low);
            }
        }
        Ok(required.is_empty())
    }

    /// `wp_kses_attr_check()`: `None` where PHP returns false (and blanks its by-reference arguments).
    pub fn attr_check(
        &self,
        name: &[u8],
        value: &[u8],
        whole: &[u8],
        vless: bool,
        element: &[u8],
        allowed: &AllowedHtml,
    ) -> Result<Option<CheckedAttr>, KsesError> {
        let Some(tag) = allowed.get(&lower(element)) else {
            return Ok(None);
        };
        let none;
        let attrs = match tag {
            AllowedAttrs::Attrs(a) => a,
            AllowedAttrs::Empty => {
                none = ByteMap::default();
                &none
            }
        };
        Ok(match self.verdict(attrs, &lower(name), value, vless)? {
            Verdict::Rejected => None,
            Verdict::AsWritten => Some(CheckedAttr {
                value: value.to_vec(),
                whole: whole.to_vec(),
            }),
            Verdict::Restyled(encoded) => Some(CheckedAttr {
                whole: replace_all(whole, value, &encoded),
                value: encoded,
            }),
        })
    }

    /// The check [`KsesApi::attr_check`] makes, given the element's attribute rules and the attribute's lowercase
    /// name.
    fn verdict(
        &self,
        attrs: &ByteMap<AttrRules>,
        name_low: &[u8],
        value: &[u8],
        vless: bool,
    ) -> Result<Verdict, KsesError> {
        // WordPress copies the `data-*` rules under the attribute's own name before the directive check; the copy
        // cannot change that check (the rules are already in the map, under `data-*`), so nothing is copied here.
        let rules = match attrs.get(name_low) {
            Some(r) if *r != AttrRules::Denied => r,
            _ => match attrs.get(&b"data-*"[..]).filter(|r| !rules_empty(r)) {
                Some(r) if is_data_name(name_low) => r,
                _ => return Ok(Verdict::Rejected),
            },
        };
        if name_low.starts_with(b"data-wp-bind--")
            && directive_binds_value_restricted_attribute(name_low, attrs)
        {
            return Ok(Verdict::Rejected);
        }
        let mut verdict = Verdict::AsWritten;
        if name_low == b"style" {
            let decoded = decode_attribute(value);
            let filtered = self.safecss_filter_attr(&decoded)?;
            if filtered.is_empty() || filtered == b"0" {
                return Ok(Verdict::Rejected);
            }
            if filtered != *decoded {
                verdict = Verdict::Restyled(self.d.escaper.esc_attr(&filtered)?);
            }
        }
        if let AttrRules::Rules(list) = rules {
            let value = match &verdict {
                Verdict::Restyled(encoded) => encoded,
                _ => value,
            };
            for rule in list {
                if !check_attr_val(value, vless, rule)? {
                    return Ok(Verdict::Rejected);
                }
            }
        }
        Ok(verdict)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_place_value_replacement_agrees_with_str_replace() {
        let cases: &[(&[u8], &[u8])] = &[
            (b"style", b"color:red;"),
            (b"style", b"s"),
            (b"style", b"le=\"x"),
            (b"style", b"="),
            (b"style", b"st"),
            (b"style", b"e=\"e"),
            (b"style", b""),
            (b"style", b"a\"b"),
        ];
        for (name, value) in cases {
            let mut whole = Vec::new();
            write_whole(&mut whole, name, value);
            let expected = replace_all(&whole, value, b"NEW");
            if value_only_in_place(&whole, name, value) {
                let mut fast = Vec::new();
                write_whole(&mut fast, name, b"NEW");
                assert_eq!(fast, expected, "{:?}", String::from_utf8_lossy(value));
            }
        }
        assert!(value_only_in_place(
            b"style=\"color:red;\"",
            b"style",
            b"color:red;"
        ));
        assert!(!value_only_in_place(b"style=\"s\"", b"style", b"s"));
    }

    #[test]
    fn angle_brackets_are_dropped_after_the_mark() {
        let mut buf = b"<a href=\"<x>\" title=\"a>b<\">".to_vec();
        drop_angle_brackets(&mut buf, 2);
        assert_eq!(buf, b"<a href=\"x\" title=\"ab\"");
        let mut none = b"<a b".to_vec();
        drop_angle_brackets(&mut none, 2);
        assert_eq!(none, b"<a b");
    }

    #[test]
    fn short_needles_are_removed_like_str_replace() {
        for s in [
            &b"<!--x-->"[..],
            b"<!--<!---->-->",
            b"<<!--!--",
            b"---->",
            b"",
            b"<!-",
            b"a<!--b<!--c",
        ] {
            for needle in [&b"<!--"[..], b"-->"] {
                assert_eq!(remove_short(s, needle), replace_all(s, needle, b""));
            }
        }
    }
}
