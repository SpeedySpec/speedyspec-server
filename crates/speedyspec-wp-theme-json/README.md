# speedyspec-wp-theme-json

WordPress's theme.json engine (`WP_Theme_JSON`, `WP_Theme_JSON_Schema`) in Rust, split into domain pieces, with
`ThemeJsonApi` as the facade behind the PHP class. It follows the WordPress release every area is recorded
against (`compat/wordpress.json`). Arrays come back equal to WordPress's by PHP's `==`; CSS is byte for byte the same.

All four plans are in: the typed model, the schema tables, migration, sanitizing, construction, merging, the data
methods, stylesheets (`get_stylesheet` for every type, `get_styles_for_block`, layout styles, preset variables and
classes, fluid font sizes, `wp_get_block_css_selector`), `remove_insecure_properties`, duotone SVG filters,
`resolve_variables`, the resolver (`WP_Theme_JSON_Resolver`) and the global styles functions.

## The crates

| Crate | Holds |
|---|---|
| `speedyspec-wp-theme-json-contracts` | The typed model (`ThemeJson`, `Settings`, `StyleNode`, `Preset`, …), the value objects (`Origin`, `ByOrigin`, `Leaf`), WordPress's class constants and layout definitions as embedded data (`schema`), and the ports (`BlockMetadata`, `TextFunctions`, `Gettext`, `Diagnostics`, `SafeCss` from kses, `GlobalSettings`, `ThemeJsonFilters`, `ActiveTheme`, `Capabilities`). |
| `speedyspec-wp-theme-json` (this crate) | `ThemeJsonApi` and its messages, the codec, migration, sanitizing, construction, merging, the data methods, selectors and block metadata, setting and style nodes, presets and fluid font sizes, declarations, layout, the stylesheet, PHP's numeric rules, and memory adapters. No PHP. |
| `speedyspec-wp-theme-json-php` | `WP_Theme_JSON`, `WP_Theme_JSON_Schema`, `WP_Theme_JSON_Data` and `wp_get_block_css_selector()` as thin PHP over a per-request handle arena; adapters onto WordPress's block registries, text, translation, notice, kses, global settings and theme-support functions, and the Rust Hook API for filters. |

## The pieces

| Module | WordPress | Job |
|---|---|---|
| `selectors` | `split_selector_list`, `append_to_selector`, `scope_selector`, `get_blocks_metadata`, `wp_get_block_css_selector`, … | Selector lists (only split, joined and prefixed, so they stay strings) and each block's computed selectors, cached on the registries' change marker. |
| `nodes` | `get_setting_nodes`, `get_style_nodes`, `get_block_nodes` | The nodes in WordPress's order (responsive, pseudo-selector and custom-state nodes included), as WordPress's node arrays, through `wp_theme_json_get_style_nodes`. |
| `presets` | `compute_preset_vars`, `compute_preset_classes`, `flatten_tree`, `_wp_to_kebab_case`, `wp_get_typography_font_size_value` | Preset variables and classes, `custom` variables, and fluid font sizes. |
| `declarations` | `compute_style_properties`, `get_property_value`, `get_feature_declarations_for_node`, `resolve_variables`, … | Declarations for a node, refs, the style engine's background slice, feature selectors, the separator, button width and paragraph indent adjustments, and variables resolved to values. |
| `layout` | `get_layout_styles`, `get_root_layout_rules` | Layout rules from WordPress's layout definitions, kept only when kses allows them. |
| `security` | `remove_insecure_properties`, `remove_insecure_styles`, `remove_insecure_settings`, `is_safe_css_declaration`, … | Only what kses, the escapers and the user's capabilities allow (`styles.css` needs `edit_css`). |
| `duotone` | `get_svg_filters`, `WP_Duotone::get_filter_svg_from_preset` and its `colord_*` parsing | The SVG filters for duotone presets; the block supports area keeps `WP_Duotone` itself. |
| `resolver` | `WP_Theme_JSON_Resolver` | Core, blocks, theme (parent under child) and user layers, read, translated by `theme-i18n.json`, filtered (`wp_theme_json_data_*`) and kept per request as WordPress's statics are; `theme.json` files cached per worker by path, size and modification time; style variations; resolved file URIs. |
| `global_styles` | `wp_get_global_settings`, `wp_get_global_styles`, `wp_get_global_stylesheet`, … | The global styles functions over the resolver, with WordPress's object cache calls (group `theme_json`); `ResolverGlobalSettings` gives fluid typography `wp_get_global_settings()`. |
| `php_json` | `json_decode` | PHP's JSON decoding: key order, integer keys, integers, its error messages. |
| `stylesheet` | `get_stylesheet`, `get_styles_for_block`, `process_pseudo_selectors`, `process_blocks_custom_css`, … | The stylesheet, put together part by part over a `Sheet` (the document, the ports, the blocks and their metadata). |

## Patterns, and where they are used

- **Facade:** `ThemeJsonApi`, one method per public method of `WP_Theme_JSON`.
- **Command and Query:** every method is a message on `speedyspec-cqrs`'s buses, bound by
  `register_theme_json_handlers`, so PHP can override one by rebinding its key.
- **Data Mapper:** `ThemeJsonCodec` maps WordPress's array form to the typed model and back. Every struct keeps
  keys it does not know, and every leaf is a `Leaf` (`Typed` or `Raw`), so nothing WordPress would keep is lost.
- **Chain of Responsibility:** schema migration, one `MigrationStep` per version (v1 → v2 → v3).
- **Specification:** the sanitizer's schema (`sanitize::Spec`), built per document from the schema tables, the
  registered blocks, elements, variations and breakpoints, shared copy-on-write as WordPress's array copies are.
- **Strategy:** the preset rules applied while merging (`UseDefaultNames`, `RemoveDefaultSlugs`), and fluid font
  sizes (`FluidFontSize`) behind `PRESETS_METADATA`'s `value_func` and `font-size` declarations.
- **Builder:** the stylesheet, appended part by part in WordPress's order (`Sheet::stylesheet`).
- **Repository per origin, behind a Facade:** `ThemeJsonResolver` reads each layer from its source (files, the
  registered blocks, the user's post) through ports, and `GlobalStylesApi` sits over it.
- **Adapter:** the memory adapters here, `HookApiFilters` over the Rust Hook API; the PHP adapters in the `-php`
  crate.
- **Null Object:** `NoFilters`, `NoThemeSupports`, `NoCapabilities`, `NoTranslation`.

Operations whose WordPress semantics are array algebra (migrate, sanitize, construct, merge) run on the array form
and decode into the model at the end; the model is the stored form later readers use. The stylesheet pieces read
the array form too, because WordPress's code is `_wp_array_get` over loose arrays and its style nodes are arrays a
filter may change.

## The deliberate differences

1. **Key order.** Arrays returned to PHP are equal by `==`, not `===`: struct fields come back in the schema's order;
   maps keyed by data (blocks, elements, variations, states) keep WordPress's order. `get_data()` is sorted exactly
   as WordPress sorts it.
2. **Protected members.** The PHP classes keep WordPress's public methods and constants. A subclass overriding a
   protected method does not change what Rust computes, and protected members reached through Reflection do not
   exist.
3. **A failing `wp_theme_json_get_style_nodes` callback** surfaces as a PHP `Error` carrying the hook's message,
   not as the callback's own exception.
4. **Duotone filters** are always the compact markup WordPress prints without `SCRIPT_DEBUG`.
5. **Resolver results are new objects.** WordPress's resolver returns its static objects, so a caller that changes
   `get_core_data()`'s result changes WordPress's cache; the port hands out a new `WP_Theme_JSON` each time.
6. **The protected statics of `WP_Theme_JSON_Resolver`** (`$core`, `$blocks_cache`, `$i18n_schema`, …) do not exist,
   so tests that reach them through Reflection do not apply.
7. **`get_custom_css()`** (deprecated) reads block selectors from the computed metadata; WordPress reads its static
   cache, which is empty when nothing filled it earlier in the request.

## Testing

- Unit and integration tests whose expected values were produced by WordPress itself (sanitizing, unwrapping,
  opting in, breakpoints, construction, spacing sizes, merging, the data methods, PHP's float formatting and
  `round()`).
- Stylesheet tests the same way: selectors and block metadata, nodes, fluid font sizes, kebab-casing, preset
  variables and classes, declarations, layout, and whole stylesheets (core's `theme.json`, Twenty Twenty-Five,
  responsive breakpoints, variations, the style nodes filter) compared byte for byte. They bind the Rust kses port
  for `safecss_filter_attr()`.
- Security, duotone and variable tests the same way (every colour notation, kses-rewritten CSS, HTML in preset
  names, `edit_css`).
- Resolver and global styles tests the same way, over nine of WordPress's test themes (`tests/data/themedir1`),
  classic themes with theme supports, user styles, data filters and the object cache keys.
- `compat/theme-json`: 424 recorded scenarios (the class constants, migration, the public API, the core
  `theme.json`, Twenty Twenty-Three to Twenty-Five with style variations, the PHPUnit `themedir1` themes, and each
  of their stylesheets by type, insecure-property removal, SVG filters, resolved variables and
  `wp_filter_global_styles_post`, the resolver over the test themes, and each default theme's global settings, styles,
  stylesheets and style variations with and without user styles), replayed through the native classes by `speedyspec-wp-theme-json-php`'s `compat`
  test.
- `benches/stylesheet.rs` and `benches/php-baseline.php`; the numbers are in [docs/benchmarks.md](docs/benchmarks.md).

## Not done yet

- The area is not in the runtime's worker list: the worker loads WordPress's own files and runs no area's
  `prepare_release` yet (kses, shortcodes and i18n are in the same state), and WordPress's theme.json class files
  are not guarded, so wiring the area now would declare `WP_Theme_JSON` twice. `release.rs` is ready for it.
- WordPress's PHPUnit theme tests (`wpThemeJson.php`, `wpThemeJsonResolver.php`, …) are not yet run against the area.
- No per-worker stylesheet or merged-data cache: a hit would skip PHP callbacks the work reaches (kses and escaper
  filters, data filters, notices). WordPress's own per-request statics and object cache calls are kept.
