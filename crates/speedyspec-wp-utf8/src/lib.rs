//! WordPress's UTF-8 helpers over bytes.
//!
//! Each function has a reference implementation, a line-for-line port of WordPress's pure-PHP fallback, and where it
//! pays off a fast path that must agree with it. The public natives in `speedyspec-wp-utf8-php` choose between them
//! to match what WordPress returns on PHP 8.5 with `mbstring` and PCRE.

mod codepoints;
mod latin1;
mod mb;
mod scan;
mod scrub;
mod validate;

pub use codepoints::{codepoint_count, codepoint_span};
pub use latin1::{utf8_decode_latin1, utf8_encode_latin1};
pub use mb::{is_utf8_charset, mb_strlen, mb_substr, php_substr};
pub use scan::{Scan, scan_utf8};
pub use scrub::{check_invalid_utf8, has_noncharacters, scrub_utf8};
pub use validate::{is_valid_utf8, is_valid_utf8_reference};
