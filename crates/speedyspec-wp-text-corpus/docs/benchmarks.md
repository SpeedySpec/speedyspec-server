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

## Natives in PHP (2026-11-21, plan 5 branch)

`SPEEDYSPEC_TEXT_BENCH=1 cargo test --release -p speedyspec-wp-text-php --test bench_php`: each native formatting
function called from PHP, against WordPress 7.1.5's own formatting.php loaded a second time in `namespace Legacy`, in
the same embedded PHP over the same settings and hooks (compat/formatting/setup.php in port mode). Inputs are the
corpus's: `short` is each function's recorded non-whole-post inputs (less the crafted PCRE backtrack cases, which
measure the documented backtrack-limit break), `whole_posts` and `page` as above. Native and legacy outputs are
checked equal before each is timed; each rate is the best of three 0.3-second runs. The `the_content` chain is
`apply_filters()` with formatting's five callbacks as default-filters.php registers them (`wptexturize`, `wpautop`,
`shortcode_unautop` at 10, `capital_P_dangit` at 11, `convert_smilies` at 20).

`convert_chars()`, `normalize_whitespace()`, `force_balance_tags()`, `wp_strip_all_tags()`, `shortcode_unautop()`
and `wp_rel_nofollow()` on text without links take a zero-copy path: the argument is read in place and returned as
the same string when unchanged. For the four of them that are bound use cases (`convert_chars`, `wp_strip_all_tags`,
`shortcode_unautop`, `wp_rel_nofollow`), the path first checks that the handler hasn't been rebound in the container.

Machine: Apple M4 Max, rustc 1.99.0 release, PHP 8.5.12 static embed. Load average 9 to 15 during the two
back-to-back runs; ranges below are from those runs (MiB/s).

| Function | Group | Native | Legacy PHP | Native / PHP |
|---|---|---|---|---|
| `esc_html` | short | 31.8 to 34.2 | 23.7 to 26.4 | 1.30 to 1.34x |
|  | whole_posts | 506.4 to 556.1 | 153.3 to 169.8 | 3.28 to 3.30x |
|  | page | 462.1 to 509.9 | 157.8 to 170.7 | 2.93 to 2.99x |
| `esc_attr` | short | 9.6 to 10.4 | 7.9 to 8.6 | 1.21 to 1.22x |
|  | whole_posts | 519.3 to 559.6 | 153.9 to 169.5 | 3.30 to 3.37x |
|  | page | 460.7 to 506.9 | 161.3 to 171.1 | 2.86 to 2.96x |
| `esc_textarea` | short | 2.2 to 2.3 | 2.5 to 2.7 | 0.85 to 0.86x |
|  | whole_posts | 465.0 to 501.3 | 161.3 to 178.4 | 2.81 to 2.88x |
|  | page | 553.9 to 608.5 | 162.5 to 181.1 | 3.36 to 3.41x |
| `esc_url` | short | 14.8 | 4.8 to 5.0 | 2.98 to 3.10x |
| `sanitize_key` | short | 12.6 to 13.1 | 11.5 to 12.6 | 1.04 to 1.10x |
| `sanitize_title` | short | 8.5 to 8.7 | 8.4 | 1.02 to 1.03x |
| `sanitize_title_with_dashes` | short | 20.3 to 21.0 | 8.3 to 8.4 | 2.42 to 2.54x |
| `sanitize_text_field` | short | 23.6 to 25.1 | 17.3 to 18.7 | 1.34 to 1.37x |
|  | whole_posts | 190.0 to 200.4 | 139.8 to 149.8 | 1.34 to 1.36x |
|  | page | 188.2 to 196.4 | 148.8 to 160.1 | 1.18 to 1.32x |
| `sanitize_file_name` | short | 2.7 to 2.9 | 1.9 to 2.1 | 1.30 to 1.52x |
| `sanitize_html_class` | short | 6.8 to 7.0 | 6.4 to 6.5 | 1.04 to 1.09x |
| `is_email` | short | 20.0 to 21.5 | 15.8 to 17.6 | 1.22 to 1.27x |
| `sanitize_email` | short | 9.7 to 10.4 | 8.0 to 8.2 | 1.22 to 1.27x |
| `remove_accents` | short | 37.8 to 39.6 | 34.6 to 36.4 | 1.04 to 1.14x |
|  | whole_posts | 458.8 to 494.5 | 253.1 to 265.9 | 1.81 to 1.86x |
|  | page | 350.6 to 406.4 | 220.8 to 230.0 | 1.52 to 1.84x |
| `wptexturize` | short | 8.4 to 9.0 | 6.1 | 1.38 to 1.47x |
|  | whole_posts | 491.3 to 498.2 | 126.6 to 131.1 | 3.80 to 3.88x |
|  | page | 2748.3 to 2881.2 | 726.4 to 745.7 | 3.78 to 3.86x |
| `wpautop` | short | 392.4 to 461.9 | 179.5 to 187.7 | 2.09 to 2.57x |
|  | whole_posts | 294.6 to 295.4 | 70.5 to 80.4 | 3.67 to 4.18x |
|  | page | 225.9 to 235.6 | 55.3 to 59.7 | 3.78 to 4.26x |
| `shortcode_unautop` | short | 32.5 to 34.5 | 26.9 | 1.20 to 1.28x |
|  | whole_posts | 42931.1 to 47733.7 | 7308.2 to 7690.6 | 5.87 to 6.21x |
|  | page | 62026.9 to 63758.5 | 7488.0 to 7681.7 | 8.28 to 8.30x |
| `capital_P_dangit` | short | 54.6 to 56.3 | 44.7 to 48.5 | 1.16 to 1.22x |
|  | whole_posts | 4903.2 to 5379.5 | 727.7 to 789.5 | 6.74 to 6.81x |
|  | page | 5699.0 to 6138.5 | 654.6 to 704.2 | 8.09 to 9.38x |
| `convert_chars` | short | 189.9 to 199.3 | 82.1 to 89.7 | 2.12 to 2.43x |
|  | whole_posts | 72944.5 to 83312.5 | 27671.8 to 30467.3 | 2.64 to 2.73x |
|  | page | 75358.0 to 81566.4 | 27364.2 to 27495.1 | 2.74 to 2.98x |
| `force_balance_tags` | short | 13.8 to 14.8 | 9.8 to 11.3 | 1.31 to 1.40x |
|  | whole_posts | 90.2 to 101.0 | 47.1 to 48.2 | 1.91 to 2.10x |
|  | page | 98.3 to 104.4 | 19.4 to 24.7 | 4.23 to 5.08x |
| `wp_strip_all_tags` | short | 150.6 to 168.7 | 215.9 to 222.5 | 0.68 to 0.76x |
|  | whole_posts | 904.4 to 983.0 | 536.5 to 603.2 | 1.63 to 1.69x |
|  | page | 1499.7 to 1660.0 | 981.6 to 1077.5 | 1.53 to 1.54x |
| `convert_smilies` | short | 3582.8 to 3951.5 | 1750.4 to 1880.3 | 2.05 to 2.10x |
|  | whole_posts | 1266.4 to 1464.1 | 142.7 to 144.8 | 8.87 to 10.11x |
|  | page | 1609.5 to 1614.5 | 124.3 to 145.4 | 11.07 to 12.99x |
| `make_clickable` | short | 75.8 to 82.2 | 39.1 to 43.9 | 1.87 to 1.94x |
|  | whole_posts | 477.5 to 514.7 | 68.5 to 79.3 | 6.49 to 6.97x |
|  | page | 478.5 to 508.9 | 72.1 to 76.0 | 6.64 to 6.70x |
| `wp_rel_nofollow` | short | 10.2 to 12.0 | 6.2 to 6.8 | 1.64 to 1.77x |
|  | whole_posts | 1240.3 to 1372.2 | 1190.7 to 1293.0 | 1.04 to 1.06x |
|  | page | 1147.2 to 1363.2 | 1221.2 to 1257.0 | 0.94 to 1.08x |
| `the_content` chain | whole_posts | | | 5.11 to 5.20x |
|  | page | | | 4.43 to 4.70x |

Targets (spec): every function at least as fast as PHP on every input, and the `the_content` chain at least 5x on
whole posts. Met, with these exceptions:
- **`esc_textarea` on short inputs runs at about 0.85x.** WordPress's own body calls `get_option( 'blog_charset' )`
  on every call, and the native has to make that call through PHP. On a value of a few bytes, the round trip costs
  more than the escaping. On whole posts it is about 3x.
- **`wp_strip_all_tags` on short inputs runs at about 0.7x.** On values of a few bytes the cost is in the call
  itself. The check that the handler is still the text API's own (two lookups in the container per call) costs
  about a fifth: without it the fast path ran at 0.96x to 1.00x. On whole posts it is about 1.7x.
- **`wp_rel_nofollow` is level with PHP** (0.94x to 1.08x on whole posts and the page). Its time goes on slashing
  and unslashing the whole text, which PHP also does in C.
- **The `the_content` chain is at 5.1x to 5.2x on whole posts** (target met), **and 4.4x to 4.7x on the long page.**
  `wpautop` is still its largest cost.

`bench_php` gates on these numbers. Every function must reach 0.95x of PHP (a 5% margin for noise on a loaded
machine), `wp_rel_nofollow` 0.9x, `esc_textarea` on short inputs 0.8x, `wp_strip_all_tags` on short inputs 0.65x,
and the chain 3.5x.
