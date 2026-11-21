//! A security oracle for the kses port: whether sanitized HTML could run script in a browser or carry an XML or SVG
//! bomb onward.
//!
//! The output is parsed as a browser parses a fragment of `<body>` (html5ever, scripting on), walked for script
//! vectors, serialized and parsed again to catch mutation XSS, and scanned as XML (quick-xml, which never expands
//! entities or fetches anything) for declarations and processing instructions.
//!
//! Two levels: [`Level::Exploit`] where script would run or a bomb survives, [`Level::Policy`] for what is unusual
//! but cannot run (an unlisted but harmless URL scheme, an inert declaration remnant).

mod html;
mod xml;

use speedyspec_wp_kses_contracts::{AllowedHtml, Protocols};

/// How serious a finding is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Exploit,
    Policy,
}

/// One thing the oracle found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub level: Level,
    pub rule: &'static str,
    pub detail: String,
}

impl Finding {
    pub(crate) fn exploit(rule: &'static str, detail: impl Into<String>) -> Finding {
        Finding {
            level: Level::Exploit,
            rule,
            detail: detail.into(),
        }
    }

    pub(crate) fn policy(rule: &'static str, detail: impl Into<String>) -> Finding {
        Finding {
            level: Level::Policy,
            rule,
            detail: detail.into(),
        }
    }
}

/// Inspects one kses output against the allowlist and protocols it was sanitized with. `xml` also requires the
/// output to be well-formed as an XML fragment, for output meant for XML.
pub fn inspect(
    output: &[u8],
    allowed: &AllowedHtml,
    protocols: &Protocols,
    xml: bool,
) -> Vec<Finding> {
    let mut findings = html::inspect(output, allowed, protocols);
    findings.extend(xml::inspect(output, xml));
    findings.dedup();
    findings
}

/// The first exploit in an output, described, for the corpus and fuzz gates.
pub fn first_exploit(
    output: &[u8],
    allowed: &AllowedHtml,
    protocols: &Protocols,
) -> Option<String> {
    inspect(output, allowed, protocols, false)
        .into_iter()
        .find(|f| f.level == Level::Exploit)
        .map(|f| format!("{}: {}", f.rule, f.detail))
}
