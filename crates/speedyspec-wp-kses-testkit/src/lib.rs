//! Test support for the kses port.
//!
//! [`PhpOracle`] runs WordPress's own kses.php, from the pinned release, in a PHP process, so every Rust function can
//! be compared byte for byte with the code it ports. [`allowed_from_php`] reads a PHP `$allowed_html` array with the
//! same `isset`/`empty`/`is_array` rules kses applies.

mod convert;
mod gate;
mod oracle;
mod release;

pub use convert::{allowed_from_php, bytes, from_bytes, rule_value};
pub use gate::{Verdict, gate};
pub use oracle::{OracleResult, PhpOracle};
pub use release::{WORDPRESS, wordpress_release};

mod corpus;

pub use corpus::{CORPUS_SETS, corpus, css_cases, posts, protocols_value, xss_attacks};
