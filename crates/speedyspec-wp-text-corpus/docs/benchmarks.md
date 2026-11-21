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
