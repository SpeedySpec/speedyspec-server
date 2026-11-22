# Benchmarks

Median time per call. Rust: `cargo bench -p speedyspec-wp-theme-json --bench stylesheet` (criterion, release
profile). PHP: `php benches/php-baseline.php <wordpress 7.1.5>` (WordPress 7.1.5's own classes, PHP 8.5 CLI
without opcache). Both use every core block of 7.1.5 and the theme merged into core's `theme.json`, with
`include_block_style_variations`. Apple silicon laptop, 2026-11-22. Numbers only; no design decision hangs on
them.

| Operation | Rust | PHP 7.1.5 |
|---|---:|---:|
| twentytwentythree/construct | 622 µs | 210 µs |
| twentytwentythree/merge_into_core | 288 µs | 62 µs |
| twentytwentythree/get_stylesheet/variables | 1265 µs | 277 µs |
| twentytwentythree/get_stylesheet/styles | 7306 µs | 1322 µs |
| twentytwentythree/get_stylesheet/presets | 483 µs | 206 µs |
| twentytwentythree/get_stylesheet/custom-css | 328 µs | 54 µs |
| twentytwentyfour/construct | 842 µs | 259 µs |
| twentytwentyfour/merge_into_core | 304 µs | 64 µs |
| twentytwentyfour/get_stylesheet/variables | 1007 µs | 325 µs |
| twentytwentyfour/get_stylesheet/styles | 8647 µs | 1631 µs |
| twentytwentyfour/get_stylesheet/presets | 693 µs | 247 µs |
| twentytwentyfour/get_stylesheet/custom-css | 333 µs | 63 µs |
| twentytwentyfive/construct | 691 µs | 227 µs |
| twentytwentyfive/merge_into_core | 251 µs | 57 µs |
| twentytwentyfive/get_stylesheet/variables | 784 µs | 311 µs |
| twentytwentyfive/get_stylesheet/styles | 4712 µs | 1490 µs |
| twentytwentyfive/get_stylesheet/presets | 374 µs | 236 µs |
| twentytwentyfive/get_stylesheet/custom-css | 249 µs | 64 µs |
| core/construct | 498 µs | 219 µs |
| core/get_stylesheet/default | 786 µs | 590 µs |

The port is slower than PHP today, by about 1.3× to 5×. The likely costs, none profiled yet:

- Every call encodes the typed model back to WordPress's array form (`ThemeJsonCodec::encode`).
- The array form is a `Vec` of pairs with linear key lookup, and paths are built as fresh strings at every step.
- `is_safe_css_declaration` runs kses for every layout declaration (PHP does too, but its kses is C-backed
  `preg_*`).

The spec's per-worker stylesheet cache (plan 4) removes repeat work entirely; tuning the engine itself is a
follow-up.
