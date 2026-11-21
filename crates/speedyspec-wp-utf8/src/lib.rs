//! WordPress's UTF-8 helpers over bytes.
//!
//! Each function has a reference implementation, a line-for-line port of WordPress's pure-PHP fallback, and where it
//! pays off a fast path that must agree with it. The public natives in `speedyspec-wp-utf8-php` choose between them
//! to match what WordPress returns on PHP 8.5 with `mbstring` and PCRE.

mod scan;
mod validate;

pub use scan::{Scan, scan_utf8};
pub use validate::{is_valid_utf8, is_valid_utf8_reference};
