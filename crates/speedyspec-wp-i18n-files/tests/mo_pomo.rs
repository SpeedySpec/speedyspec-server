//! MO for pomo and the translation file writers, against WordPress (recorded in `data/mo-recorded.json` by
//! `data/record-mo.php`).

use std::path::{Path, PathBuf};

use md5::{Digest, Md5};
use serde_json::Value;
use speedyspec_wp_i18n_files::mo_pomo::{
    MoPairs, mo_pack, mo_pairs, translation_file_mo, translation_file_php, var_export_string,
};

fn data() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data")
}

fn recorded() -> Value {
    serde_json::from_str(&std::fs::read_to_string(data().join("mo-recorded.json")).unwrap())
        .unwrap()
}

fn unhex(v: &Value) -> Vec<u8> {
    let s = v.as_str().unwrap_or("");
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn md5_hex(b: &[u8]) -> String {
    hex(&Md5::digest(b))
}

fn file(name: &str) -> PathBuf {
    match name.split_once('/') {
        Some(("data", f)) => data().join(f),
        Some(("mo", f)) => data().join("mo").join(f),
        Some((dir @ ("pomo" | "l10n"), f)) => {
            data().join("../../../../tests/php/data").join(dir).join(f)
        }
        _ => panic!("{name}"),
    }
}

/// A recorded list (as written, or as the md5 of its canonical form) against `items`, each item's parts already
/// strings.
fn same_list(want: &Value, items: &[Vec<String>], what: &str) {
    match want.get("md5") {
        Some(digest) => {
            let canonical = items
                .iter()
                .map(|i| i.join(":"))
                .collect::<Vec<_>>()
                .join("\n");
            assert_eq!(
                md5_hex(canonical.as_bytes()),
                digest.as_str().unwrap(),
                "{what}"
            );
        }
        None => {
            let wanted: Vec<Vec<String>> = want
                .as_array()
                .unwrap()
                .iter()
                .map(|item| {
                    item.as_array()
                        .unwrap()
                        .iter()
                        .map(|p| match p {
                            Value::Bool(b) => {
                                if *b {
                                    "1".into()
                                } else {
                                    "0".into()
                                }
                            }
                            Value::Number(n) => n.to_string(),
                            other => other.as_str().unwrap().to_owned(),
                        })
                        .collect()
                })
                .collect();
            assert_eq!(items, wanted.as_slice(), "{what}");
        }
    }
}

fn same_bytes(want: &Value, got: &[u8], what: &str) {
    match want.get("md5") {
        Some(digest) => assert_eq!(md5_hex(got), digest.as_str().unwrap(), "{what}"),
        None => assert_eq!(hex(got), want.as_str().unwrap(), "{what}"),
    }
}

/// `[key, is_int, value]` triples as `(key bytes, value bytes)`.
fn keyed(v: &Value) -> Vec<(Vec<u8>, Vec<u8>)> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|t| {
            let key = if t[1].as_bool().unwrap() {
                t[0].as_i64().unwrap().to_string().into_bytes()
            } else {
                unhex(&t[0])
            };
            (key, unhex(&t[2]))
        })
        .collect()
}

#[test]
fn pairs_match_recorded_imports() {
    for (name, case) in recorded()["files"].as_object().unwrap() {
        let bytes = std::fs::read(file(name)).unwrap();
        let got = mo_pairs(&bytes);
        match got {
            MoPairs::Pairs(pairs) => {
                assert!(
                    case["result"].as_bool().unwrap(),
                    "{name}: WordPress returned false"
                );
                let events: Vec<Vec<String>> = pairs
                    .iter()
                    .map(|(o, t)| {
                        if o.is_empty() {
                            vec!["header".into(), hex(t)]
                        } else {
                            vec!["pair".into(), hex(o), hex(t)]
                        }
                    })
                    .collect();
                same_list(&case["events"], &events, name);
            }
            MoPairs::False => assert!(
                !case["result"].as_bool().unwrap(),
                "{name}: WordPress imported it"
            ),
            MoPairs::ZeroLengthRead => panic!("{name}: no recorded file reads zero bytes"),
        }
    }
}

#[test]
fn an_empty_table_is_a_zero_length_read() {
    // Magic, revision 0, no entries: WordPress's reader then calls fread() with length 0, which throws.
    let mut bytes = 0x950412de_u32.to_le_bytes().to_vec();
    for v in [0u32, 0, 28, 28, 0, 28] {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    assert_eq!(mo_pairs(&bytes), MoPairs::ZeroLengthRead);
    assert_eq!(mo_pairs(b"\xde\x12"), MoPairs::False);
}

#[test]
fn pack_matches_recorded_exports() {
    for (name, case) in recorded()["files"].as_object().unwrap() {
        if case["pack"].get("md5").is_some() {
            // Rebuild the packed strings from the file, as WordPress's export would after import.
            continue;
        }
        let pairs: Vec<(Vec<u8>, Vec<u8>)> = case["pack"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| (unhex(&p[0]), unhex(&p[1])))
            .collect();
        same_bytes(
            &case["export"],
            &mo_pack(&pairs, &unhex(&case["headers"])),
            name,
        );
    }
}

#[test]
fn transform_outputs_match() {
    for (name, case) in recorded()["files"].as_object().unwrap() {
        if case.get("file_headers").is_none()
            || !case["file_error"].is_null()
            || case["file_entries"].get("md5").is_some()
        {
            continue;
        }
        let headers = keyed(&case["file_headers"]);
        let entries = keyed(&case["file_entries"]);
        same_bytes(
            &case["transform_mo"],
            &translation_file_mo(&headers, &entries, false),
            &format!("{name} to mo"),
        );
        same_bytes(
            &case["transform_php"],
            &translation_file_php(&headers, &entries),
            &format!("{name} to php"),
        );
    }
}

#[test]
fn var_export_escapes_like_php() {
    let rec = recorded();
    for case in rec["var_export"].as_array().unwrap() {
        match case[0].as_str().unwrap() {
            "keys" => {
                let entries: Vec<(Vec<u8>, Vec<u8>)> = [
                    ("0", "zero"),
                    ("01", "leading"),
                    ("-1", "negative"),
                    ("1.5", "float"),
                    ("k", "string"),
                    ("7", "int"),
                ]
                .iter()
                .map(|(k, v)| (k.as_bytes().to_vec(), v.as_bytes().to_vec()))
                .collect();
                let mut want = b"<?php\nreturn ['messages'=>".to_vec();
                want.extend_from_slice(&unhex(&case[1]));
                want.extend_from_slice(b"];\n");
                assert_eq!(translation_file_php(&[], &entries), want);
            }
            "list" => {}
            value => assert_eq!(
                var_export_string(&unhex(&Value::String(value.into()))),
                unhex(&case[1]),
                "{value}"
            ),
        }
    }
}

#[test]
#[cfg_attr(debug_assertions, ignore = "timing needs a release build")]
fn translation_file_export_is_linear() {
    let entries: Vec<(Vec<u8>, Vec<u8>)> = (0..40_000)
        .map(|i| {
            (
                format!("Original string number {i}").into_bytes(),
                format!("Übersetzung {i}").into_bytes(),
            )
        })
        .collect();
    let headers = vec![(b"Language".to_vec(), b"de_DE".to_vec())];
    let start = std::time::Instant::now();
    let mo = translation_file_mo(&headers, &entries, false);
    let php = translation_file_php(&headers, &entries);
    let took = start.elapsed();
    assert!(mo.len() > 40_000 && php.len() > 40_000);
    assert!(took.as_millis() < 60, "exported in {took:?}");
}

#[test]
fn an_empty_original_in_the_entries_takes_the_header_slot() {
    // array_merge( array( '' => $headers ), $entries ): an entry keyed '' replaces the header string in place.
    let entries = vec![
        (Vec::new(), b"replaced".to_vec()),
        (b"a".to_vec(), b"b".to_vec()),
    ];
    let out = translation_file_mo(&[(b"X".to_vec(), b"y".to_vec())], &entries, false);
    assert!(out.windows(9).any(|w| w == b"replaced\0"));
    assert!(!out.windows(5).any(|w| w == b"X: y\n"));
}
