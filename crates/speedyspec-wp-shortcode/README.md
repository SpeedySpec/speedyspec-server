# speedyspec-wp-shortcode

WordPress's Shortcode API (`wp-includes/shortcodes.php`) in Rust: `add_shortcode()`, `do_shortcode()`,
`strip_shortcodes()`, `shortcode_parse_atts()`, `shortcode_atts()` and the rest. The output is byte for byte the
same as WordPress's, apart from the two deliberate differences below.

## The crates

| Crate | Holds |
|---|---|
| `speedyspec-wp-shortcode-contracts` | Plain types (`TagName`, `TagSet`, `ParsedAtts`, `ShortcodeMatch`), the ports and strategy traits, and WordPress's pattern strings (`get_shortcode_regex`, `get_shortcode_atts_regex`, `shortcode_tag_alternation`, `preg_quote`). It depends on nothing of ours except the hook value type. |
| `speedyspec-wp-shortcode` (this crate) | `ShortcodeApi`, the scanners, the attribute parsers, `MemoryRegistry`, `HookApiFilters`, `NoFilters`, and the use-case messages. |
| `speedyspec-wp-shortcode-php` | The PHP functions, native over `ShortcodeApi`; `PhpGlobalRegistry` over `$shortcode_tags`; `PhpShortcodeHandler`; `RustHtmlSplitter` onto `speedyspec-wp-html-split`; adapters onto WordPress's own `_doing_it_wrong()` and kses. |

Other areas read the registered names through the `ShortcodeTagNames` port: `wpautop`, `shortcode_unautop` and
`wptexturize`. They never depend on the registry.

## Patterns, and where they are used

- **Strategy:**
  - `ShortcodeScanner` has two implementations: `HandScanner`, the default (SIMD through `memchr` and
    `aho-corasick`), and `FancyRegexScanner`, the literal pattern run by `fancy-regex`.
  - `AttsParser` also has two: `HandAttsParser` and `RegexAttsParser`, which uses the `regex` crate's DFA.
  - [docs/benchmarks.md](docs/benchmarks.md) gives the numbers behind the defaults.
- **Iterator:** a scan yields matches lazily as spans, left to right, without overlap, as `preg_replace_callback()`
  sees them. It holds no borrow of the scanner, so handlers can re-enter it.
- **Registry** (PoEAA): `ShortcodeRegistry`, implemented by `MemoryRegistry` and, in the PHP runtime, by
  `PhpGlobalRegistry` over the `$shortcode_tags` global itself.
- **Adapter:**
  - `PhpShortcodeHandler` wraps a PHP callable.
  - `HookApiFilters` connects the filters to the Rust Hook API.
  - `RustHtmlSplitter` connects `HtmlSplitter` to `speedyspec-wp-html-split`.
  - The `legacy` adapters connect to WordPress's PHP.
- **Facade:** `ShortcodeApi` has one method per WordPress function. The PHP functions are one-line shims onto it.
- **Command and Query:** every function is a message on `speedyspec-cqrs`'s buses, bound by
  `register_shortcode_handlers`, so PHP can override one by binding its key.
- **Value Object:** `TagName`, `TagSet`, `ParsedAtts` and `ShortcodeMatch`.
- **Null Object:** `NoFilters`, for when nothing listens.

## Tests

```bash
cargo test -p speedyspec-wp-shortcode-contracts -p speedyspec-wp-shortcode
cargo test --release -p speedyspec-wp-shortcode --test scan_cases -- --ignored   # linear-time check
cargo test -p speedyspec-wp-shortcode-php --test php        # the PHP bridge
cargo test -p speedyspec-wp-shortcode-php --test compat     # recorded compatibility, all strategy combinations
cargo test -p speedyspec-wp-shortcode-php --test wordpress  # WordPress's tests/phpunit/tests/shortcode.php
cargo bench -p speedyspec-wp-shortcode
```

- **Scanner and parser tables:** the expected values in `tests/support/*_cases.rs` were taken from PHP 8.5 and
  WordPress's own `shortcodes.php`.
- **Fuzzing:** `scan_diff` and `atts_diff` fuzz each fast strategy against its reference.
- **Recorded compatibility:** `compat/shortcode` holds 206 scenarios recorded from WordPress 7.1.5. They come from
  WordPress's shortcode tests, their data providers, the whole-post test data, and edge cases. They replay through
  all four combinations of scanner and parser.

## Deliberate differences from WordPress

These two are also listed in `compat/shortcode/breaks.json`.

1. **No backtrack-limit failure.** On some content, WordPress's PCRE runs out of JIT stack or hits
   `pcre.backtrack_limit`. `preg_replace_callback()` then returns `null`, and `do_shortcode()` returns `""`. An example
   is an opener followed by 20,000 `"/ "` pairs. The port's scanners cannot fail this way, so they return the real
   result.
2. **The C locale only.** After `setlocale( LC_CTYPE, … )` away from `"C"`, PHP's PCRE builds locale tables, and
   `\w`, `\s` and `\S` can match bytes at or above 0x80. The port always uses the C locale's classes. The runtime
   never changes `LC_CTYPE`, so this shows only if a plugin calls `setlocale()`.

## Not yet covered by the WordPress test run

WordPress's tests in `media.php`, `post/output.php` and `widgets/wpWidgetText.php` that touch shortcodes need a
database, posts and a booted WordPress. So does `test_smilies_arent_converted`, which is marked
`needs-wordpress-core`. They run once a full-WordPress harness exists.
