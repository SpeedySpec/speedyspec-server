# speedyspec-wp-theme-json-php

`WP_Theme_JSON`, `WP_Theme_JSON_Schema` and `WP_Theme_JSON_Data` as PHP classes over the Rust engine, and
`wp_get_block_css_selector()` when WordPress has not defined it. Each
`WP_Theme_JSON` holds a handle into a per-request arena; its methods are native functions that dispatch through the
container to `ThemeJsonApi`. `WP_Theme_JSON_Data` is WordPress's own thin class, unchanged. The ports are adapted
onto WordPress's PHP: the block registries (read again only when they change, never forgetting a block within a
request, and taking new styles of known blocks only on a read that finds no new blocks, exactly as WordPress's
static cache does), `sanitize_title()` and the escapers, `__()`, the notice functions, `safecss_filter_attr()`,
`wp_get_global_settings()` and `current_theme_supports()`. Filters go through the Rust Hook API, so
`ThemeJsonArea::new` takes the `HookApi`, as the shortcode area does.

The area is not yet in the runtime's default areas or `release.rs`; that comes with the resolver (plan 4), because a
class cannot be replaced half-ported. See [`speedyspec-wp-theme-json`](../speedyspec-wp-theme-json/README.md).
