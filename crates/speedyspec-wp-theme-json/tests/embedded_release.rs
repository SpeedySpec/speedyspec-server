//! The schema files speedyspec-wp-theme-json-contracts embeds come from the WordPress release the theme-json area is
//! recorded from (`compat/theme-json/source.json`, or the pinned release in `compat/wordpress.json`).

use std::path::PathBuf;

use speedyspec_wp_theme_json_contracts::schema::DATA_RELEASE;

/// The catalogue: `$SPEEDYSPEC_COMPAT_DIR`, which `.cargo/config.toml` sets to this repository's `compat/`.
fn compat_dir() -> PathBuf {
    std::env::var_os("SPEEDYSPEC_COMPAT_DIR")
        .map(PathBuf::from)
        .expect("no compat catalogue: set SPEEDYSPEC_COMPAT_DIR")
}

fn json(path: PathBuf) -> serde_json::Value {
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn the_embedded_files_come_from_the_theme_json_areas_release() {
    let compat = compat_dir();
    let source = json(compat.join("theme-json/source.json"));
    let table = json(compat.join("wordpress.json"));
    let release = source["wordpress"].as_str().or(table["wordpress"].as_str());
    assert_eq!(
        release,
        Some(DATA_RELEASE),
        "the theme-json area is recorded from WordPress {release:?}, not {DATA_RELEASE}: re-export the \
         wordpress-<release>.* files in speedyspec-platform and release speedyspec-wp-theme-json-contracts"
    );
}
