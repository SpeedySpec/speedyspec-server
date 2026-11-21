//! kses.php's pure string functions, which need no hooks, over fast byte matchers.
//!
//! [`scan`] holds one `#[inline]` matcher per kses regex; the standalone functions here and the fused scanner in
//! `speedyspec-wp-kses` are both built from them, so no regex quirk is written twice. Everything works on bytes:
//! kses's regexes run without `/u`, so `\s` is PCRE's ASCII set and invalid UTF-8 passes as in PHP.

pub mod scan;
