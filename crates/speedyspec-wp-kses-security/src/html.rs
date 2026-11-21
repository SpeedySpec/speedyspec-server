//! The HTML side of the oracle.

use html5ever::serialize::{SerializeOpts, TraversalScope, serialize};
use html5ever::tendril::TendrilSink;
use html5ever::{ParseOpts, QualName, local_name, ns, parse_fragment};
use markup5ever_rcdom::{Handle, NodeData, RcDom, SerializableHandle};
use speedyspec_wp_kses_contracts::{AllowedHtml, Protocols};

use crate::Finding;

/// Elements that run script, load documents or change how the page is read, whatever their attributes.
const DANGEROUS_ELEMENTS: &[&str] = &[
    "script",
    "iframe",
    "frame",
    "frameset",
    "object",
    "embed",
    "applet",
    "base",
    "meta",
    "link",
    "style",
    "portal",
    "noscript",
    "template",
    "xmp",
    "plaintext",
    "noembed",
    "noframes",
    "param",
    "handler",
    "listener",
    "foreignobject",
];

/// Attributes that hold a URL a browser follows or loads.
const URL_ATTRIBUTES: &[&str] = &[
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
    "lowsrc",
    "dynsrc",
    "ping",
    "srcset",
    "to",
    "from",
    "values",
    "by",
];

/// SVG animation elements that can rewrite another attribute, `href` among them.
const ANIMATION_ELEMENTS: &[&str] = &[
    "animate",
    "set",
    "animatemotion",
    "animatetransform",
    "discard",
];

/// Content types a `data:` URL runs script with.
const SCRIPTABLE_DATA: &[&str] = &[
    "text/html",
    "application/xhtml+xml",
    "image/svg+xml",
    "text/xml",
    "application/xml",
    "text/javascript",
    "application/javascript",
    "text/xsl",
];

fn parse(output: &[u8]) -> RcDom {
    let text = String::from_utf8_lossy(output);
    let context = QualName::new(None, ns!(html), local_name!("body"));
    parse_fragment(
        RcDom::default(),
        ParseOpts::default(),
        context,
        vec![],
        true,
    )
    .one(text.as_ref())
}

fn reserialize(dom: &RcDom) -> Vec<u8> {
    let root = dom.document.children.borrow().first().cloned();
    let mut out = Vec::new();
    if let Some(root) = root {
        let opts = SerializeOpts {
            traversal_scope: TraversalScope::ChildrenOnly(None),
            ..SerializeOpts::default()
        };
        let _ = serialize(&mut out, &SerializableHandle::from(root), opts);
    }
    out
}

/// The scheme a browser reads from a URL value: whitespace and C0 controls trimmed, tabs and newlines removed.
fn scheme(value: &str) -> Option<String> {
    let cleaned: String = value
        .trim_matches(|c: char| c <= ' ')
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect();
    let colon = cleaned.find(':')?;
    let candidate = &cleaned[..colon];
    let valid = candidate
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic())
        && candidate
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    valid.then(|| candidate.to_ascii_lowercase())
}

fn is_script_url(value: &str) -> bool {
    let cleaned: String = value
        .trim_matches(|c: char| c <= ' ')
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect();
    match scheme(value).as_deref() {
        Some("javascript" | "vbscript" | "livescript" | "mocha") => true,
        Some("data") => {
            let mime = cleaned[5..]
                .split([';', ','])
                .next()
                .unwrap_or_default()
                .trim()
                .to_ascii_lowercase();
            SCRIPTABLE_DATA.contains(&mime.as_str())
        }
        _ => false,
    }
}

fn walk(node: &Handle, allowed: &AllowedHtml, protocols: &Protocols, findings: &mut Vec<Finding>) {
    if let NodeData::Element {
        name,
        attrs,
        template_contents,
        ..
    } = &node.data
    {
        let tag: String = name.local.to_ascii_lowercase().to_string();
        let tag_allowed = allowed.get(tag.as_bytes()).is_some();
        if DANGEROUS_ELEMENTS.contains(&tag.as_str()) && !tag_allowed {
            findings.push(Finding::exploit("element", format!("<{tag}>")));
        }
        for attr in attrs.borrow().iter() {
            let attr_name: String = attr.name.local.to_ascii_lowercase().to_string();
            let value: &str = &attr.value;
            if attr_name.starts_with("on") {
                findings.push(Finding::exploit(
                    "event-handler",
                    format!("<{tag} {attr_name}>"),
                ));
            }
            if attr_name == "srcdoc" {
                findings.push(Finding::exploit("srcdoc", format!("<{tag} srcdoc>")));
            }
            if ANIMATION_ELEMENTS.contains(&tag.as_str()) && attr_name == "attributename" {
                let target = value.trim().to_ascii_lowercase();
                if target == "href" || target.ends_with(":href") {
                    findings.push(Finding::exploit(
                        "element",
                        format!("<{tag} attributeName={target}>"),
                    ));
                }
            }
            if URL_ATTRIBUTES.contains(&attr_name.as_str()) {
                if is_script_url(value) {
                    findings.push(Finding::exploit(
                        "script-url",
                        format!("<{tag} {attr_name}={value:?}>"),
                    ));
                } else if let Some(s) = scheme(value) {
                    let listed = protocols
                        .0
                        .iter()
                        .any(|p| p.eq_ignore_ascii_case(s.as_bytes()));
                    let is_url_like = !matches!(
                        attr_name.as_str(),
                        "to" | "from" | "values" | "by" | "xmlns"
                    );
                    if !listed && is_url_like {
                        findings.push(Finding::policy(
                            "unlisted-scheme",
                            format!("<{tag} {attr_name}={value:?}>"),
                        ));
                    }
                }
            }
            if attr_name == "style" {
                let lower = value.to_ascii_lowercase();
                for needle in [
                    "expression(",
                    "javascript:",
                    "vbscript:",
                    "-moz-binding",
                    "behavior:",
                    "behaviour:",
                ] {
                    if lower.contains(needle) {
                        findings.push(Finding::exploit(
                            "style",
                            format!("<{tag} style={value:?}>"),
                        ));
                    }
                }
            }
        }
        if let Some(contents) = template_contents.borrow().as_ref() {
            walk(contents, allowed, protocols, findings);
        }
    }
    for child in node.children.borrow().iter() {
        walk(child, allowed, protocols, findings);
    }
}

fn findings_of(dom: &RcDom, allowed: &AllowedHtml, protocols: &Protocols) -> Vec<Finding> {
    let mut findings = Vec::new();
    walk(&dom.document, allowed, protocols, &mut findings);
    findings
}

/// The HTML findings, including any that appear only when the output is serialized and parsed again.
pub(crate) fn inspect(output: &[u8], allowed: &AllowedHtml, protocols: &Protocols) -> Vec<Finding> {
    let first = parse(output);
    let mut findings = findings_of(&first, allowed, protocols);
    let second = parse(&reserialize(&first));
    for f in findings_of(&second, allowed, protocols) {
        if f.level == crate::Level::Exploit && !findings.contains(&f) {
            findings.push(Finding::exploit(
                "mxss",
                format!("after reparsing: {}: {}", f.rule, f.detail),
            ));
        }
    }
    findings
}
