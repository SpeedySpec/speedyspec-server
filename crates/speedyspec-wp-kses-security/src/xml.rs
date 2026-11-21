//! The XML side of the oracle: declarations, processing instructions and entity references, read with quick-xml,
//! which never expands entities or resolves anything external.

use quick_xml::Reader;
use quick_xml::events::Event;

use crate::Finding;

/// The declarations XML would act on: case-sensitive, as XML is.
const DECLARATIONS: &[&[u8]] = &[
    b"<!DOCTYPE",
    b"<!ENTITY",
    b"<!ATTLIST",
    b"<!ELEMENT",
    b"<!NOTATION",
];

/// Named references any XML or HTML consumer resolves without a DTD.
const PREDEFINED: &[&[u8]] = &[b"amp", b"lt", b"gt", b"quot", b"apos"];

/// Comments in the output, whose contents are inert.
fn comment_spans(output: &[u8]) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut at = 0;
    while let Some(i) = memchr::memmem::find(&output[at..], b"<!--") {
        let start = at + i;
        let end = memchr::memmem::find(&output[start + 4..], b"-->")
            .map_or(output.len(), |e| start + 4 + e + 3);
        spans.push((start, end));
        at = end;
    }
    spans
}

pub(crate) fn inspect(output: &[u8], well_formed: bool) -> Vec<Finding> {
    let mut findings = Vec::new();
    let comments = comment_spans(output);
    let in_comment = |p: usize| comments.iter().any(|(s, e)| p > *s && p < *e);
    for decl in DECLARATIONS {
        let mut at = 0;
        while let Some(i) = memchr::memmem::find(&output[at..], decl) {
            let p = at + i;
            if !in_comment(p) {
                findings.push(Finding::exploit(
                    "declaration",
                    String::from_utf8_lossy(decl).into_owned(),
                ));
            }
            at = p + decl.len();
        }
        let lower = decl.to_ascii_lowercase();
        let lowered = output.to_ascii_lowercase();
        let mut at = 0;
        while let Some(i) = memchr::memmem::find(&lowered[at..], &lower) {
            let p = at + i;
            if !in_comment(p) && &output[p..p + decl.len()] != *decl {
                findings.push(Finding::policy(
                    "inert-declaration",
                    String::from_utf8_lossy(&output[p..p + decl.len()]).into_owned(),
                ));
            }
            at = p + decl.len();
        }
    }
    let mut at = 0;
    while let Some(i) = memchr::memmem::find(&output[at..], b"<?") {
        let p = at + i;
        if !in_comment(p) {
            findings.push(Finding::exploit(
                "declaration",
                "processing instruction".to_string(),
            ));
        }
        at = p + 2;
    }
    let mut at = 0;
    while let Some(i) = memchr::memchr(b'&', &output[at..]) {
        let p = at + i + 1;
        let name_len = output[p..]
            .iter()
            .take_while(|b| b.is_ascii_alphanumeric())
            .count();
        if name_len > 0 && output.get(p + name_len) == Some(&b';') && !in_comment(p) {
            let name = &output[p..p + name_len];
            let known = PREDEFINED.contains(&name) || speedyspec_known_html_entity(name);
            if !known {
                findings.push(Finding::exploit(
                    "unknown-entity",
                    format!("&{};", String::from_utf8_lossy(name)),
                ));
            }
        }
        at = p;
    }
    let mut reader = Reader::from_reader(output);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            // quick-xml reads `<!doctype` in any case; XML acts only on `<!DOCTYPE`, which the case-sensitive
            // scan above already reports, so a declaration event here adds nothing but a policy note.
            Ok(Event::DocType(_)) => {
                findings.push(Finding::policy("inert-declaration", "doctype".to_string()))
            }
            Ok(Event::PI(_)) | Ok(Event::Decl(_)) => {
                findings.push(Finding::exploit(
                    "declaration",
                    "processing instruction".to_string(),
                ));
            }
            Ok(Event::Eof) => break,
            Err(e) if well_formed => {
                findings.push(Finding::policy("not-well-formed", e.to_string()));
                break;
            }
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    findings
}

/// Whether a name is an HTML named character reference a browser resolves.
fn speedyspec_known_html_entity(name: &[u8]) -> bool {
    let mut reference = name.to_vec();
    reference.push(b';');
    speedyspec_wp_html_syntax::named_reference(&reference, 0)
        .is_some_and(|(_, len)| len == reference.len())
}
