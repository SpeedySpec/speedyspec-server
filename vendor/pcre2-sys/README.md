# pcre2-sys (SpeedySpec's patch)

A drop-in for crates.io's [`pcre2-sys`](https://github.com/BurntSushi/rust-pcre2) 0.2.10, wired in by
`[patch.crates-io]` in the workspace manifest, so the `pcre2` crate (and so `speedyspec-wp-text`'s patterns)
runs on it.

## Why

The server binary embeds the static PHP, and PHP bundles its own PCRE2. PHP renames only PCRE2's public API
(`php_pcre2_*`, in `main/php_compat.h`); the internals keep their stock names (`_pcre2_ucd_records_8`,
`_pcre2_default_tables_8`, `_pcre2_valid_utf_8`, ...), and so do a few public functions PHP doesn't rename.
The upstream `pcre2-sys` builds a second PCRE2 with the same names. In a binary that links both, the linker
keeps one copy of each, and PHP's PCRE2 reads the other build's tables: `preg_split('/([^\w])/u', 'a b')`
fails with "Internal error". `crates/speedyspec-wp-text/tests/php_pcre2.rs` reproduces it.

## What is different from upstream

- `upstream/` is PCRE2 **10.44**, copied from the static PHP's `php-src/ext/pcre/pcre2lib` (PHP 8.5), the
  same version PHP's `preg_*` functions run, so the Rust patterns and PHP share one PCRE2's semantics. The
  layout is PHP's (`sljit/` next to the sources). `upstream/include/config.h` is ours: PHP's build settings
  without its PHP includes. Each source file's header carries PCRE2's BSD licence.
- `prefix.h` is force-included into every file and renames every external symbol to `speedyspec_*`.
  `src/bindings.rs` (upstream's, generated for 10.45) links each function to its prefixed name with
  `#[link_name]`; functions newer than 10.44 are declared but don't exist, so don't call them.
- The build never uses a system libpcre2, and `PCRE2_SYS_STATIC` no longer matters.
- `src/tests.rs` checks the version and that every defined symbol has the prefix.

## Updating

When PHP's bundled PCRE2 changes (a new PHP version, `scripts/php-version.sh`), copy `ext/pcre/pcre2lib/*.c`, `*.h` and `sljit/`
over `upstream/src` (`pcre2.h` into `upstream/include`; not PHP's `config.h`), compare `config.h`'s values
with PHP's, update `PCRE2_MINOR` in `src/bindings.rs`, then regenerate the prefix list from the workspace root:

    vendor/pcre2-sys/update-prefix

and run `cargo test -p pcre2-sys -p speedyspec-wp-text`.
