//! Test support for the kses port.
//!
//! [`PhpOracle`] runs WordPress's own kses.php, from the pinned release, in a PHP process, so every Rust function can
//! be compared byte for byte with the code it ports. [`allowed_from_php`] reads a PHP `$allowed_html` array with the
//! same `isset`/`empty`/`is_array` rules kses applies.

mod bench;
mod bombs;
mod convert;
mod gate;
mod generate;
mod oracle;
mod release;

pub use bench::{bench_page, bench_snippets};
pub use bombs::{BombGenerator, bomb_generators};
pub use convert::{allowed_from_php, bytes, from_bytes, rule_value};
pub use gate::{ExploitFinder, Verdict, gate};
pub use generate::token_soup;
pub use oracle::{OracleResult, PHP_TIMED_OUT, PhpOracle};
pub use release::{wordpress, wordpress_release};

mod corpus;

pub use corpus::{CORPUS_SETS, corpus, css_cases, posts, protocols_value, xss_attacks};
