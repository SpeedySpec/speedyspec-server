//! kses.php's pure string functions, which need no hooks, over fast byte matchers.
//!
//! [`scan`] holds one `#[inline]` matcher per kses regex; the standalone functions here and the fused scanner in
//! `speedyspec-wp-kses` are both built from them, so no regex quirk is written twice. Everything works on bytes:
//! kses's regexes run without `/u`, so `\s` is PCRE's ASCII set and invalid UTF-8 passes as in PHP.

mod entities;
mod entity_names;
mod protocol;
pub mod scan;
mod text;

pub use entities::{
    decode_entities, default_html_entity_names, default_xml_entity_names, normalize_entities,
    valid_unicode,
};
pub use protocol::{bad_protocol, bad_protocol_once, bad_protocol_once2};
pub use text::{array_lc, html_error, no_null, stripslashes};
