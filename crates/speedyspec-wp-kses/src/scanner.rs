//! The fused scanner: `wp_kses()` in one pass over the bytes per call, with `wp_kses_split2()`, `wp_kses_attr()`
//! and `wp_kses_attr_check()` handled in place as each token is met.

use std::borrow::Cow;
use std::rc::Rc;

use speedyspec_hooks::Value;
use speedyspec_wp_html_syntax::decode_attribute;
use speedyspec_wp_kses_contracts::{
    AllowedAttrs, AllowedHtml, AllowedSpec, AttrRule, AttrRules, EntityContext, KsesError,
    Protocols,
};
use speedyspec_wp_kses_lexical::scan::{
    TokenKind, invalid_wrapper, next_token, tag_head, xhtml_slash,
};
use speedyspec_wp_kses_lexical::{check_attr_val, hair, no_null, normalize_entities, stripslashes};

use crate::KsesApi;
use crate::api::{bytes_value, php_string};
use crate::convert::allowed_to_value;

/// The most `wp_kses()` passes a comment or invalid-markup token gets to settle. Every corpus input settles within
/// 2; a token still changing after this many is one PHP never finishes, and the port drops it.
const MAX_PASSES: usize = 16;

/// An attribute that passed `wp_kses_attr_check()`: its value and whole text, as the check left them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckedAttr {
    pub value: Vec<u8>,
    pub whole: Vec<u8>,
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
pub fn restricts_attribute_values(attrs: &indexmap::IndexMap<Box<[u8]>, AttrRules>) -> bool {
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
    attrs: &indexmap::IndexMap<Box<[u8]>, AttrRules>,
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
    fn hook<'a>(
        &self,
        content: Cow<'a, [u8]>,
        allowed: &AllowedSpec,
        protocols: &Protocols,
    ) -> Result<Cow<'a, [u8]>, KsesError> {
        let allowed_value = || match allowed {
            AllowedSpec::Context(c) => bytes_value(c.as_bytes()),
            AllowedSpec::Explicit(a) => allowed_to_value(a),
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
            let inner = replace_all(&replace_all(&token, b"<!--", b""), b"-->", b"");
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
        if allowed_html.get(&elem.to_ascii_lowercase()).is_none() {
            return Ok(());
        }
        if head.slash {
            out.extend_from_slice(b"</");
            out.extend_from_slice(elem);
            out.push(b'>');
            return Ok(());
        }
        self.attr_into(elem, attrlist, &allowed_html, protocols, out)
    }

    fn resolve_allowed(&self, allowed: &AllowedSpec) -> Result<Rc<AllowedHtml>, KsesError> {
        match allowed {
            AllowedSpec::Context(c) => self.allowed_html(c),
            AllowedSpec::Explicit(a) => Ok(a.clone()),
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
        self.attr_into(element, attr, &allowed_html, protocols, &mut out)?;
        Ok(out)
    }

    fn attr_into(
        &self,
        element: &[u8],
        attr: &[u8],
        allowed: &AllowedHtml,
        protocols: &Protocols,
        out: &mut Vec<u8>,
    ) -> Result<(), KsesError> {
        let slash: &[u8] = if xhtml_slash(attr) { b" /" } else { b"" };
        let element_low = element.to_ascii_lowercase();
        let attrs = match allowed.get(&element_low) {
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
        let parsed = hair(attr, protocols, &uris);
        let mut required: Vec<&[u8]> = attrs
            .iter()
            .filter(|(_, r)| r.is_required())
            .map(|(n, _)| &**n)
            .collect();
        let stripped = |out: &mut Vec<u8>| {
            if slash.is_empty() {
                out.push(b'<');
                out.extend_from_slice(element);
                out.push(b'>');
            }
        };
        let mut kept = Vec::new();
        for a in &parsed {
            let name_low = a.name.to_ascii_lowercase();
            let is_required = required.iter().any(|r| **r == *name_low);
            match self.attr_check(&a.name, &a.value, &a.whole, a.vless, element, allowed)? {
                Some(checked) => {
                    kept.push(b' ');
                    kept.extend_from_slice(&checked.whole);
                    if is_required {
                        required.retain(|r| **r != *name_low);
                    }
                }
                None if is_required => {
                    stripped(out);
                    return Ok(());
                }
                None => {}
            }
        }
        if !required.is_empty() {
            stripped(out);
            return Ok(());
        }
        out.push(b'<');
        out.extend_from_slice(element);
        out.extend(kept.into_iter().filter(|b| *b != b'<' && *b != b'>'));
        out.extend_from_slice(slash);
        out.push(b'>');
        Ok(())
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
        let name_low = name.to_ascii_lowercase();
        let Some(tag) = allowed.get(&element.to_ascii_lowercase()) else {
            return Ok(None);
        };
        let mut attrs = match tag {
            AllowedAttrs::Attrs(a) => a.clone(),
            AllowedAttrs::Empty => indexmap::IndexMap::new(),
        };
        let rules = match attrs.get(&name_low[..]) {
            Some(r) if *r != AttrRules::Denied => r.clone(),
            _ => {
                let data = attrs
                    .get(&b"data-*"[..])
                    .filter(|r| !rules_empty(r))
                    .cloned();
                match data {
                    Some(r) if is_data_name(&name_low) => {
                        attrs.insert(name_low.clone().into(), r.clone());
                        r
                    }
                    _ => return Ok(None),
                }
            }
        };
        if name_low.starts_with(b"data-wp-bind--")
            && directive_binds_value_restricted_attribute(&name_low, &attrs)
        {
            return Ok(None);
        }
        let mut value = value.to_vec();
        let mut whole = whole.to_vec();
        if name_low == b"style" {
            let decoded = decode_attribute(&value).into_owned();
            let filtered = self.safecss_filter_attr(&decoded)?;
            if filtered.is_empty() || filtered == b"0" {
                return Ok(None);
            }
            if filtered != decoded {
                let encoded = self.d.escaper.esc_attr(&filtered)?;
                whole = replace_all(&whole, &value, &encoded);
                value = encoded;
            }
        }
        if let AttrRules::Rules(list) = &rules {
            for rule in list {
                if !check_attr_val(&value, vless, rule)? {
                    return Ok(None);
                }
            }
        }
        Ok(Some(CheckedAttr { value, whole }))
    }
}
