# speedyspec-wp-theme-json

WordPress's theme.json engine (`WP_Theme_JSON`, `WP_Theme_JSON_Schema`) in Rust, split into domain pieces, with
`ThemeJsonApi` as the facade behind the PHP class. It follows WordPress 7.1.5, the release every area is recorded
against. Arrays come back equal to WordPress's by PHP's `==`; CSS (plan 2) will be byte for byte the same.

This is plan 1 of 4: the typed model, the schema tables, migration, sanitizing, construction, merging and the
data methods. Stylesheets, security and the resolver follow.

## The crates

| Crate | Holds |
|---|---|
| `speedyspec-wp-theme-json-contracts` | The typed model (`ThemeJson`, `Settings`, `StyleNode`, `Preset`, …), the value objects (`Origin`, `ByOrigin`, `Leaf`), WordPress's class constants as embedded data (`schema`), and the ports (`BlockMetadata`, `TextFunctions`, `Gettext`, `Diagnostics`). |
| `speedyspec-wp-theme-json` (this crate) | `ThemeJsonApi` and its messages, the codec, migration, sanitizing, construction, merging, the data methods, PHP's numeric rules, and memory adapters. No PHP. |
| `speedyspec-wp-theme-json-php` | `WP_Theme_JSON`, `WP_Theme_JSON_Schema` and `WP_Theme_JSON_Data` as thin PHP classes over a per-request handle arena; adapters onto WordPress's block registries, text, translation and notice functions. |

## Patterns, and where they are used

- **Facade:** `ThemeJsonApi`, one method per public method of `WP_Theme_JSON`.
- **Command and Query:** every method is a message on `speedyspec-cqrs`'s buses, bound by
  `register_theme_json_handlers`, so PHP can override one by rebinding its key.
- **Data Mapper:** `ThemeJsonCodec` maps WordPress's array form to the typed model and back. Every struct keeps
  keys it does not know, and every leaf is a `Leaf` (`Typed` or `Raw`), so nothing WordPress would keep is lost.
- **Chain of Responsibility:** schema migration, one `MigrationStep` per version (v1 → v2 → v3).
- **Specification:** the sanitizer's schema (`sanitize::Spec`), built per document from the schema tables, the
  registered blocks, elements, variations and breakpoints, shared copy-on-write as WordPress's array copies are.
- **Strategy:** the preset rules applied while merging (`UseDefaultNames`, `RemoveDefaultSlugs`).
- **Adapter:** the memory adapters here; the PHP adapters in the `-php` crate.

Operations whose WordPress semantics are array algebra (migrate, sanitize, construct, merge) run on the array form
and decode into the model at the end; the model is the stored form later readers use.

## The deliberate differences

1. **Key order.** Arrays returned to PHP are equal by `==`, not `===`: struct fields come back in the schema's order;
   maps keyed by data (blocks, elements, variations, states) keep WordPress's order. `get_data()` is sorted exactly
   as WordPress sorts it.
2. **Protected members.** The PHP classes keep WordPress's public methods and constants. A subclass overriding a
   protected method does not change what Rust computes, and protected members reached through Reflection do not
   exist.

## Testing

- Unit and integration tests whose expected values were produced by WordPress 7.1.5 itself (sanitizing, unwrapping,
  opting in, breakpoints, construction, spacing sizes, merging, the data methods, PHP's float formatting and
  `round()`).
- `compat/theme-json`: 127 recorded scenarios (the class constants, migration, the public API, the core
  `theme.json`, Twenty Twenty-Three to Twenty-Five with style variations, and the PHPUnit `themedir1` themes),
  replayed through the native classes by `speedyspec-wp-theme-json-php`'s `compat` test.
