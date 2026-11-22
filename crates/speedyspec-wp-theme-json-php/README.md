# speedyspec-wp-theme-json-php

`WP_Theme_JSON`, `WP_Theme_JSON_Schema` and `WP_Theme_JSON_Data` as PHP classes over the Rust engine. Each
`WP_Theme_JSON` holds a handle into a per-request arena; its methods are native functions that dispatch through the
container to `ThemeJsonApi`. `WP_Theme_JSON_Data` is WordPress's own thin class, unchanged. The ports are adapted
onto WordPress's PHP: the block registries (read again only when they change, and never forgetting a block within a
request, as WordPress's static cache does), `sanitize_title()` and the escapers, `__()`, and the notice functions.

The area is not yet in the runtime's default areas or `release.rs`; that comes with the resolver (plan 4), because a
class cannot be replaced half-ported. See [`speedyspec-wp-theme-json`](../speedyspec-wp-theme-json/README.md).
