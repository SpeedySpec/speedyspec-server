# speedyspec-wp-theme-json-php

`WP_Theme_JSON`, `WP_Theme_JSON_Schema`, `WP_Theme_JSON_Data` and `WP_Theme_JSON_Resolver` as PHP classes over the
Rust engine, the global styles functions, and `wp_get_block_css_selector()` when WordPress has not defined it.
`wp_add_global_styles_for_blocks` and `wp_theme_has_theme_json` stay WordPress's own PHP, over the native APIs.
`prepare_release` empties the release's theme.json class files and strips the ported global styles functions. Each
`WP_Theme_JSON` holds a handle into a per-request arena; its methods are native functions that dispatch through the
container to `ThemeJsonApi`. `WP_Theme_JSON_Data` is WordPress's own thin class, unchanged. The ports are adapted
onto WordPress's PHP: the block registries (read again only when they change, never forgetting a block within a
request, and taking new styles of known blocks only on a read that finds no new blocks, exactly as WordPress's
static cache does), `sanitize_title()` and the escapers, `__()`, the notice functions, `safecss_filter_attr()`,
`wp_get_global_settings()`, `current_theme_supports()` and `current_user_can()`. Filters go through the Rust Hook API, so
`ThemeJsonArea::new` takes the `WpHookApi`, as the shortcode area does.

The area is not yet in the runtime's worker: see the engine's README, "Not done yet". See [`speedyspec-wp-theme-json`](../speedyspec-wp-theme-json/README.md).
