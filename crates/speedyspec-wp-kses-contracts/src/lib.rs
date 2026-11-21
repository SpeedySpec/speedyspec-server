//! WordPress's kses sanitizer as contracts.
//!
//! The plain types kses works with (the allowlist `$allowed_html`, attribute rules, protocols, contexts) and the
//! ports an implementation fills, kept apart from any implementation so one can be replaced wholly or partly. All
//! text is bytes: PHP strings are bytes and kses's regexes run without `/u`, so invalid UTF-8 must pass as in PHP.

mod allowed;
mod attr;
mod error;
mod ports;
mod rules;

pub use allowed::{
    AllowedAttrs, AllowedHtml, AllowedSpec, ByteMap, Context, EntityContext, EntityNames,
    NameHasher, Protocols, UriAttributes,
};
pub use attr::Attr;
pub use error::{KsesError, Payload};
pub use ports::{
    AllowedHtmlSource, AttributeEscaper, Kses, KsesLexical, SafeCss, UnfilteredHtmlPolicy,
};
pub use rules::{AttrRule, AttrRules, RuleValue, ValueCallback};

/// An owned byte string, as a PHP string is.
pub type Bytes = Box<[u8]>;
