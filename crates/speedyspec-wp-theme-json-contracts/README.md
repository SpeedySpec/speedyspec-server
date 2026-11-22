# speedyspec-wp-theme-json-contracts

The contracts of the theme.json port: the typed model, the value objects, WordPress 7.1.5's `WP_Theme_JSON` class
constants (exported by `tools/export-constants.php` and embedded, so they cannot drift by transcription), and the
ports the engine needs from outside. It depends on nothing of ours except the hook value type and the kses
contracts. See [`speedyspec-wp-theme-json`](../speedyspec-wp-theme-json/README.md) for the whole area.
