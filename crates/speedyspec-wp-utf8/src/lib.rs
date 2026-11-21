//! WordPress's UTF-8 helpers over bytes.
//!
//! Each function has a reference implementation, a line-for-line port of WordPress's pure-PHP fallback, and where it
//! pays off a fast path that must agree with it. The public natives in `speedyspec-wp-utf8-php` choose between them
//! to match what WordPress returns on PHP 8.5 with `mbstring` and PCRE.

mod codepoints;
mod scan;
mod scrub;
mod validate;

pub use codepoints::{codepoint_count, codepoint_span};
pub use scan::{Scan, scan_utf8};
pub use scrub::{check_invalid_utf8, has_noncharacters, has_noncharacters_reference, scrub_utf8};
pub use validate::{is_valid_utf8, is_valid_utf8_reference};
