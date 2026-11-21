# Shortcode benchmarks

These results decide the default strategies, following the speed rule: SIMD where possible, else the `regex`
crate's fast paths, else plain regex. Each result is the `criterion` median of 20 samples. They were taken on an
Apple M4 Max (`memchr` and `aho-corasick` use NEON there), with Rust 1.99 in release, on 2026-11-21. The `cargo
bench -p speedyspec-wp-shortcode` command reproduces them.

## The corpus

All eight files are in `benches/corpus/`.

| File | Size | What it is |
|---|---|---|
| 01-no-bracket | 4.3 KB | Paragraphs with no `[` at all. |
| 02-brackets-no-tags | 4.6 KB | `[` everywhere, no registered tag: Markdown links, JSON, `<!--[if IE]>`, footnote marks. |
| 03-typical-post | 2.8 KB | A post with `caption`, `gallery`, `embed` and two plugin tags. |
| 04-long-post | 200 KB | About 300 shortcodes, some nested (`tabs` holding `tab` holding `button`), some escaped. |
| 05-one-megabyte | 1.06 MB | WordPress's whole-post test data (`tests/phpunit/data/formatting/whole-posts.php`), repeated. |
| 06-pathological | 220 KB | 20,000 openers without a `]` or without a closer. |
| 07-in-attributes | 39 KB | Shortcodes inside HTML attributes, which takes `do_shortcodes_in_html_tags()`'s attribute path. |
| 08-page-builder | 109 KB | Page-builder-style nesting five levels deep, each tag with seven attributes. |

The registered tags are WordPress's core ones plus common plugin and page-builder ones, 18 in all. The handlers
echo their attribute count and run `do_shortcode()` on enclosed content, so nested shortcodes are processed too.

## Scanning (`benches/scan.rs`)

`get_shortcode_regex()` matches over the tag set `do_shortcode()` would use.

| File | `HandScanner` (SIMD) | `FancyRegexScanner` | Speed-up |
|---|---|---|---|
| 01-no-bracket | 6.99 µs | 53.9 µs | 7.7× |
| 02-brackets-no-tags | 10.2 µs | 189 µs | 19× |
| 03-typical-post | 0.739 µs | 37.8 µs | 51× |
| 04-long-post | 42.5 µs | 33 ms | 780× |
| 05-one-megabyte | 16.7 µs | 16.9 ms | 1,000× |
| 06-pathological | 439 µs | 2.54 s | 5,800× |
| 07-in-attributes | 40.5 µs | 16.4 ms | 400× |
| 08-page-builder | 8.2 µs | 342 µs | 42× |

**Default: `HandScanner`.** It wins on every file, by about three orders of magnitude on long content. Two things
explain the gap:
- **No backtracking.** `memchr` jumps from `[` to `[`, and the hand-written state machine never revisits a byte it
  has passed.
- **Remembered searches.** Closer and `]` lookups are cached, which keeps file 06 linear (Review Focus 3, enforced
  by `unclosed_openers_are_linear`).

The `regex` crate cannot run this pattern at all, because it lacks lookahead and backreferences. So there is no
middle rung for scanning.

`FancyRegexScanner` stays as the reference that the hand scanner is fuzzed against. It is not a fallback for
production: on file 06 it takes seconds, because its backtracking engine retries the lazy attribute loop at every
opener.

## Attributes (`benches/atts.rs`)

`shortcode_parse_atts()` runs on every attribute text the corpus's shortcodes have. Files with no shortcodes are
skipped.

| File | `HandAttsParser` | `RegexAttsParser` (`regex` DFA) | Speed-up |
|---|---|---|---|
| 02-brackets-no-tags | 224 ns | 476 ns | 2.1× |
| 03-typical-post | 1.26 µs | 3.81 µs | 3.0× |
| 04-long-post | 26.3 µs | 63.2 µs | 2.4× |
| 05-one-megabyte | 18.1 µs | 54.4 µs | 3.0× |
| 06-pathological | 2.15 ms | 8.69 ms | 4.0× |
| 07-in-attributes | 20.4 µs | 66.8 µs | 3.3× |
| 08-page-builder | 28.3 µs | 84.5 µs | 3.0× |

**Default: `HandAttsParser`.** The attribute pattern has no lookarounds, so the `regex` crate's DFA fast path can
run it, and that is the second rung. Even so, a byte state machine that tries the nine alternatives in order is
2–4× faster. One reason is that it skips capture-group bookkeeping. `RegexAttsParser` stays as the fuzzing
reference.

## The tag prefilter (`benches/tokens.rs`)

This is `do_shortcode()`'s `preg_match_all( '@\[([^<>&/\[\]\x00-\x20=]++)@' )`.

| File | `memchr` + byte table | `regex` crate |
|---|---|---|
| 01-no-bracket | 49.6 ns | 81 ns |
| 02-brackets-no-tags | 540 ns | 1.9 µs |
| 03-typical-post | 98.7 ns | 242 ns |
| 04-long-post | 7.76 µs | 19.8 µs |
| 05-one-megabyte | 14.7 µs | 15.6 µs |
| 06-pathological | 203 µs | 673 µs |
| 07-in-attributes | 5.74 µs | 12.6 µs |
| 08-page-builder | 8.25 µs | 22.9 µs |

**Default: `memchr`** (`candidate_tokens`). It is faster everywhere and level only on file 05, where both are bound
by the search for the rare `[`.

## The whole `do_shortcode()` (`benches/do_shortcode.rs`), with PHP for context

Both Rust configurations produce identical output; the bench asserts it before timing. PHP is WordPress's own
`shortcodes.php` on PHP 8.5.12 with the PCRE JIT on and opcache off, the median of 50 runs, with the same handlers.

| File | Rust, hand + hand | Rust, fancy-regex + regex | PHP (WordPress) | Rust vs PHP |
|---|---|---|---|---|
| 01-no-bracket | 131 ns | 122 ns | 0.1 µs | about the same |
| 02-brackets-no-tags | 2.12 µs | 2.25 µs | 5.7 µs | 2.7× |
| 03-typical-post | 6.83 µs | 61.3 µs | 14.8 µs | 2.2× |
| 04-long-post | 1.55 ms | 21.1 ms | 4.17 ms | 2.7× |
| 05-one-megabyte | 0.643 ms | 14.2 ms | 4.22 ms | 6.6× |
| 06-pathological | 5.82 ms | 2.56 s | 306 ms | 53× |
| 07-in-attributes | 59.4 µs | 643 µs | 264 µs | 4.4× |
| 08-page-builder | 2.30 ms | 6.54 ms | 2.43 ms | 1.06× |

Two caveats on the PHP column:
- **Different `wp_html_split`.** The Rust benches split HTML with a `memchr` stand-in, because the real
  `wp_html_split` is another crate's (`speedyspec-wp-html-split`). PHP runs WordPress's `preg_split`. kses parsing
  is stubbed out on both sides.
- **Context only.** PHP pays its own interpreter costs, so this column is not a like-for-like engine comparison.

**Where the time goes next.** File 08 is nearly level with PHP. The scan itself is 8.2 µs of its 2.3 ms; the rest is
per-match work across about 400 nested `do_shortcode()` calls:
- Each call copies the tag names and builds the token set.
- Each match builds `$m` and the attributes as PHP values for the filters, even when `NoFilters` ignores them.

Two changes would remove most of that: building `$m` lazily only when a filter listens, and reusing the narrowed
tag set across nested calls. Neither changes behaviour, and the compatibility replay would guard both.

## Decision

| Strategy | Default | Kept as |
|---|---|---|
| Scanner | `HandScanner` (SIMD through `memchr` and `aho-corasick`) | `FancyRegexScanner`: reference and fuzz oracle |
| Attribute parser | `HandAttsParser` | `RegexAttsParser`: reference and fuzz oracle |
| Tag prefilter | `candidate_tokens` (`memchr`) | none |

`ShortcodeArea::new` uses these defaults. `ShortcodeArea::with_strategies` swaps either strategy without touching
anything else.
