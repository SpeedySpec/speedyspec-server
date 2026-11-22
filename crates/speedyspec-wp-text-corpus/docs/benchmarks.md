# Formatting benchmarks

Rust is timed with criterion (`cargo bench -p <crate>`), PHP with `php-baseline <function>` in the embedded PHP
running stock WordPress 7.1.5. Both run on the same inputs from the corpus. All throughputs are MiB/s or GiB/s
(binary units, as criterion reports).

- `whole_posts`: the 3 posts of `tests/php/wordpress/data/formatting/whole-posts.php`, about 100 KB in total.
- `page`: those posts joined and repeated to at least 150 KB.

## wp_html_split (2026-11-21, branch commit 4ce19ee)

Machine: Apple M4 Max, rustc 1.99.0 release, PHP 8.5.12 static embed (PCRE JIT). The machine was shared with
other builds (load average about 60), so the numbers vary between runs. The table gives the range of two steady
runs of each side.

| Group | Rust | PHP | Rust / PHP |
|---|---|---|---|
| whole_posts | 2.50 to 2.56 GiB/s | 0.75 to 0.99 GiB/s | about 2.5x to 3.4x |
| page | 11.9 to 12.7 GiB/s | 2.0 to 2.1 GiB/s | about 6x |

Target (spec): at least as fast as PHP on every bench input. Met.

## Content filters (2026-11-21, branch commit 3e016f5)

Rust: `cargo bench -p speedyspec-wp-text --bench content`, timed in thread CPU time. PHP:
`php-baseline <function>` (and `php-baseline the_content` for the chain), CPU-time column (`getrusage()`). Each
Rust bench first checks its output on the bench inputs against what WordPress recorded.

Machine: Apple M4 Max, rustc 1.99.0 release, PHP 8.5.12 static embed (PCRE JIT). The machine was shared with other
builds (load average 6.5 to 11), which slows both sides: on PHP, CPU time tracked wall time, so the load slowed the
cores rather than preempting the process. Each side ran twice, back to back with the other; the table gives the
range of the two runs. Ratios pair runs taken at the same time.

| Function | Group | Rust | PHP | Rust / PHP |
|---|---|---|---|---|
| `wptexturize` | whole_posts | 626 to 676 MiB/s | 153 to 162 MiB/s | about 4.1x |
| | page | 2.75 to 3.44 GiB/s | 774 to 838 MiB/s | 3.6x to 4.2x |
| `wpautop` | whole_posts | 327 to 381 MiB/s | 79 to 91 MiB/s | about 4.2x |
| | page | 247 to 281 MiB/s | 61 to 73 MiB/s | about 3.9x |
| `shortcode_unautop` | whole_posts | 62.6 to 68.8 GiB/s | 7.7 to 9.3 GiB/s | about 7.5x |
| | page | 73.6 to 81.2 GiB/s | 7.7 to 9.5 GiB/s | about 8.5x |
| `capital_P_dangit` | whole_posts | 38.3 to 42.2 GiB/s | 718 to 834 MiB/s | about 52x |
| | page | 39.7 to 44.2 GiB/s | 736 to 840 MiB/s | about 54x |
| `convert_chars` | whole_posts | 32.5 to 36.3 GiB/s | 28.3 to 30.8 GiB/s | about 1.15x |
| | page | 31.7 to 34.8 GiB/s | 16.6 to 25.4 GiB/s | 1.3x to 2x |
| `force_balance_tags` | whole_posts | 110 to 116 MiB/s | 25.5 to 28.0 MiB/s | about 4.2x |
| | page | 99 to 116 MiB/s | 12.1 to 15.3 MiB/s | 6.5x to 9.5x |
| `wp_strip_all_tags` | whole_posts | 1.19 to 1.25 GiB/s | 442 to 552 MiB/s | 2.3x to 2.8x |
| | page | 2.02 to 2.14 GiB/s | 1.06 to 1.16 GiB/s | about 1.9x |
| `the_content` chain | whole_posts | 216 to 231 MiB/s | 43.4 to 47.3 MiB/s | about 4.9x |
| | page | 198 to 202 MiB/s | 42.5 to 54.4 MiB/s | 3.7x to 4.7x |

`convert_chars` on these inputs (no `&`) is one `memchr` on both sides, so its ratio is noise around parity.
An earlier pair of runs at load average 6 gave the chain 4.45x (whole posts) and 4.15x (page).

The chain is `wptexturize`, `wpautop`, `shortcode_unautop` and `capital_P_dangit` through `TextApi`, values
converted to and from the Hook API's at each step as `the_content` would; `convert_smilies` joins it in plan 4.

Targets (spec):
- Every function at least as fast as PHP on every bench input: met.
- The chain at least 5x PHP on whole posts: not met. It is about 4.2x to 4.9x. Reaching 5x steadily needs
  `wpautop`'s twelve or so passes merged into fewer scans; until then the faster paths (`HandTexturizer`,
  `wpautop`) are checked against the PCRE2 transcriptions of WordPress's code by `tests/differential.rs`.

## Escaping (2026-11-21, branch commit 27764b1)

Rust: `cargo bench -p speedyspec-wp-text-escape --bench escape`, through `EscapeApi` with no filters registered,
timed in thread CPU time. PHP: `php-baseline esc_html` (and `esc_attr`, `esc_textarea`), CPU-time column. Both
include the filter call and `wp_check_invalid_utf8`. `short` is every recorded non-whole-post input of the
function (attribute values, snippets), so its rate is dominated by per-call cost. Each Rust bench first checks every
corpus case of its function.

Machine: Apple M4 Max, rustc 1.99.0 release, PHP 8.5.12 static embed. Load average 21 to 27 during both runs; the
table gives the range of two back-to-back runs of each side.

| Function | Group | Rust | PHP | Rust / PHP |
|---|---|---|---|---|
| `esc_html` | whole_posts | 516 to 560 MiB/s | 150 to 159 MiB/s | about 3.5x |
| | page | 463 to 506 MiB/s | 149 to 161 MiB/s | about 3.1x |
| | short | 181 to 190 MiB/s | 50 to 54 MiB/s | about 3.5x |
| `esc_attr` | whole_posts | 485 to 505 MiB/s | 149 to 162 MiB/s | 3.0x to 3.4x |
| | page | 493 to 502 MiB/s | 148 to 164 MiB/s | 3.0x to 3.3x |
| | short | 69 to 73 MiB/s | 14.0 to 14.4 MiB/s | about 5x |
| `esc_textarea` | whole_posts | 485 to 499 MiB/s | 160 to 174 MiB/s | about 3x |
| | page | 515 to 555 MiB/s | 159 to 172 MiB/s | about 3.3x |
| | short | 48 to 49 MiB/s | 19 to 20 MiB/s | about 2.5x |

Target (spec): at least as fast as PHP on every input. Met.

## Sanitizers and URLs (2026-11-21, plan 4 branch)

Rust: `cargo bench -p speedyspec-wp-text --bench sanitize` (the `short` groups) and `--bench content`
(`convert_smilies`, `make_clickable`, `wp_rel_nofollow` and the chain), through `TextApi` with
`register_text_filters` applied and no other filters, timed in thread CPU time. PHP: `php-baseline <function>`, CPU-time
column, over the same corpus inputs (setup.php registers `sanitize_title_with_dashes` on `sanitize_title` as
`default-filters.php` does). Each Rust bench first checks its function's corpus cases.

Machine: Apple M4 Max, rustc 1.99.0 release, PHP 8.5.12 static embed. Load average 8 to 13 during the runs; the table
gives the range of two back-to-back runs of each side.

| Function | Group | Rust | PHP | Rust / PHP |
|---|---|---|---|---|
| `esc_url` | short | 41 to 48 MiB/s | 15.5 to 18.9 MiB/s | 2.2x to 3.1x |
| `sanitize_title` | short | 33 to 36 MiB/s | 11.3 to 15.8 MiB/s | 2.1x to 3.2x |
| `sanitize_title_with_dashes` | short | 30 to 32 MiB/s | 10.0 to 13.4 MiB/s | 2.2x to 3.2x |
| `sanitize_file_name` | short | 3.5 to 3.8 MiB/s | 3.0 to 3.7 MiB/s | 1.0x to 1.25x |
| `sanitize_key` | short | 73 to 88 MiB/s | 44 to 62 MiB/s | 1.2x to 2.0x |
| `sanitize_text_field` | short | 77 to 79 MiB/s | 42 to 51 MiB/s | 1.5x to 1.9x |
| `sanitize_html_class` | short | 85 to 89 MiB/s | 25 to 31.5 MiB/s | 2.7x to 3.5x |
| `is_email` | short | 88 to 97 MiB/s | 50 to 58 MiB/s | 1.5x to 1.9x |
| `sanitize_email` | short | 45 to 51 MiB/s | 24 to 27 MiB/s | 1.7x to 2.1x |
| `remove_accents` | short | 62 to 65 MiB/s | 43 to 46.5 MiB/s | 1.3x to 1.5x |
| `convert_smilies` | whole_posts | 1.9 to 2.3 GiB/s | 223 to 237 MiB/s | 8x to 10x |
| | page | 1.9 to 2.1 GiB/s | 225 to 240 MiB/s | 8x to 9x |
| `make_clickable` | whole_posts | 472 to 615 MiB/s | 101 to 121 MiB/s | 3.9x to 6.1x |
| | page | 447 to 525 MiB/s | 84 to 120 MiB/s | 3.7x to 6.2x |
| `wp_rel_nofollow` | whole_posts | 1.26 to 1.28 GiB/s | 1.16 to 1.45 GiB/s | 0.9x to 1.1x |
| | page | 1.18 to 1.39 GiB/s | 1.12 to 1.43 GiB/s | 0.8x to 1.2x |
| `the_content` chain, with `convert_smilies` | whole_posts | 155 to 168 MiB/s | 32 to 39 MiB/s | 4.0x to 5.2x |
| | page | 166 to 172 MiB/s | 45 to 47 MiB/s | 3.5x to 3.9x |

Targets (spec): every function at least as fast as PHP on every input, and the `the_content` chain at least 5x on
whole posts. Every sanitizer, `esc_url`, `convert_smilies` and `make_clickable` meets the first.
`sanitize_file_name` only just meets it, and `wp_rel_nofollow` is level with PHP within the noise of a loaded
machine: its time goes on `stripslashes` and `addslashes` over the whole text, which PHP does in C as well. The chain
reaches 5x on whole posts only at the top of its range (4.0x to 5.2x) and stays under it on the long page, so the
second target is not met yet; `wpautop` is still the chain's largest cost (see "Content filters").
