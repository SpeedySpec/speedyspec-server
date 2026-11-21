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
