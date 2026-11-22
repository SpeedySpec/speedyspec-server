//! One binary links both PCRE2 builds: the static PHP's and the one under speedyspec-wp-text's patterns. Each
//! must keep its own.
//!
//! Both bundle PCRE2. PHP renames only PCRE2's public API, so if the Rust build kept the stock names, the
//! linker would keep a single copy of each internal (`_pcre2_ucd_records_8`, `_pcre2_default_tables_8`, ...)
//! and one side would read the other build's tables: PHP's `preg_split()` then fails with an internal error.
//! vendor/pcre2-sys renames every symbol of the Rust side's PCRE2.

use std::rc::Rc;

use speedyspec_hooks::Value;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, Php};
use speedyspec_wp_text::preg::{Pattern, SplitFlags, preg_match, preg_split};

fn main() {
    // The Rust side first, so its PCRE2 objects are the ones the linker meets first.
    let word = Pattern::php(r"/([^\w])/u");
    let found = preg_match(&word, "a b".as_bytes())
        .expect("the Rust match runs")
        .expect("a non-word character");
    assert_eq!(found.full(), b" ", "the Rust pattern finds the space");
    let parts = preg_split(&word, "é b".as_bytes(), -1, SplitFlags::default())
        .expect("the Rust split runs");
    assert_eq!(
        parts,
        vec!["é".as_bytes(), b"b"],
        "the Rust split knows é is a word character"
    );

    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas: Vec::new(),
    })
    .expect("PHP boots");
    let split = php
        .eval(
            "pcre2",
            r#"
            $parts = preg_split('/([^\w])/u', 'a b');
            if ($parts === false) { return 'preg_split failed: ' . preg_last_error_msg(); }
            $unicode = preg_split('/([^\w])/u', "\u{e9} b");
            if ($unicode === false) { return 'preg_split failed on UTF-8: ' . preg_last_error_msg(); }
            return implode('|', $parts) . ' ' . implode('|', $unicode);
            "#,
        )
        .expect("the PHP probe runs");
    assert_eq!(
        split,
        Value::Str("a|b é|b".to_string()),
        "PHP's preg_split works next to the Rust PCRE2"
    );

    // And the Rust side still works after PHP's PCRE2 has run.
    let after = preg_match(&word, "x-y".as_bytes())
        .expect("the Rust match runs")
        .expect("a non-word character");
    assert_eq!(after.full(), b"-");
    println!("pcre2: both PCRE2 builds work in one binary");
}
