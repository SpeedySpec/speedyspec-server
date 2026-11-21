//! The MO and `.l10n.php` readers, against what WordPress 7.1.5's own `WP_Translation_File` readers make of every
//! fixture in `data/` (recorded in `data/recorded.json` by `data/record.php`).

use std::path::{Path, PathBuf};

use base64::Engine;
use speedyspec_wp_i18n::TranslationFile;
use speedyspec_wp_i18n_contracts::{FileIdentity, ReadError, TranslationFileReader};
use speedyspec_wp_i18n_files::{ByExtension, L10nPhpReader, MoReader};

fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}

fn b64(v: &serde_json::Value) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(v.as_str().unwrap())
        .unwrap()
}

fn identity(path: &Path) -> FileIdentity {
    FileIdentity {
        path: path.to_path_buf(),
        dev: 0,
        ino: 0,
        size: 0,
        mtime_ns: 0,
    }
}

#[test]
fn every_fixture_reads_as_wordpress_reads_it() {
    let recorded: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(data("recorded.json")).unwrap()).unwrap();
    let reader = ByExtension::default();
    for (name, want) in &recorded {
        let path = data(name);
        let got = reader.read(&path);
        if name.starts_with("unsupported-") {
            assert_eq!(got, Err(ReadError::Unsupported), "{name}");
            continue;
        }
        match want["error"].as_str() {
            Some(message) => {
                let got = got.expect_err(name);
                assert_eq!(got.to_string(), message, "{name}");
                continue;
            }
            None => {
                let raw = got.unwrap_or_else(|e| panic!("{name}: {e}"));
                let headers: Vec<(Vec<u8>, Vec<u8>)> = raw
                    .headers
                    .iter()
                    .map(|(k, v)| (k.clone().into_bytes(), v.clone().into_bytes()))
                    .collect();
                let want_headers: Vec<(Vec<u8>, Vec<u8>)> = want["headers"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| (b64(&p[0]), b64(&p[1])))
                    .collect();
                assert_eq!(headers, want_headers, "{name} headers");
                let file = TranslationFile::new(identity(&path), raw);
                let entries: Vec<(Vec<u8>, Vec<u8>)> = file
                    .entries()
                    .map(|(k, v)| (k.to_vec(), v.to_vec()))
                    .collect();
                let want_entries: Vec<(Vec<u8>, Vec<u8>)> = want["entries"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| (b64(&p[0]), b64(&p[1])))
                    .collect();
                assert_eq!(entries, want_entries, "{name} entries");
            }
        }
    }
    assert!(recorded.len() >= 25);
}

#[test]
fn big_endian_reads_the_same_as_little_endian() {
    let le = MoReader.read(&data("simple.mo")).unwrap();
    let be = MoReader.read(&data("big-endian.mo")).unwrap();
    assert_eq!(le.headers, be.headers);
    assert_eq!(le.entries.len(), be.entries.len());
}

#[test]
fn l10n_php_twins_match_their_mo() {
    for (mo, php) in [
        ("example-simple.mo", "example-simple.php"),
        ("plural-complex.mo", "plural-complex.php"),
    ] {
        let a = TranslationFile::new(identity(&data(mo)), MoReader.read(&data(mo)).unwrap());
        let b = TranslationFile::new(
            identity(&data(php)),
            L10nPhpReader.read(&data(php)).unwrap(),
        );
        let mut ea: Vec<_> = a.entries().collect();
        let mut eb: Vec<_> = b.entries().collect();
        ea.sort();
        eb.sort();
        assert_eq!(ea, eb, "{mo} vs {php}");
    }
}

#[test]
fn missing_file_is_unreadable() {
    assert_eq!(MoReader.read(&data("nope.mo")), Err(ReadError::Unreadable));
}

/// The core de_DE pack's `.l10n.php` (set `SPEEDYSPEC_I18N_PACKS` to the folder the benchmark's fetch script fills).
#[test]
fn core_pack_parses() {
    let Ok(packs) = std::env::var("SPEEDYSPEC_I18N_PACKS") else {
        eprintln!("skipped: set SPEEDYSPEC_I18N_PACKS");
        return;
    };
    let php = Path::new(&packs).join("core/de_DE.l10n.php");
    let raw = L10nPhpReader.read(&php).expect("de_DE.l10n.php parses");
    let mo = MoReader
        .read(&Path::new(&packs).join("core/de_DE.mo"))
        .unwrap();
    let a = TranslationFile::new(identity(&php), raw);
    let b = TranslationFile::new(identity(&php), mo);
    assert_eq!(a.entries().filter(|(k, _)| !k.contains(&4)).count(), 3632);
    assert_eq!(a.entries().count(), b.entries().count());
}
