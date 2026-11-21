//! `WP_Token_Map` in Rust against WordPress 7.1.5's own class (recorded in `data/map-recorded.json` by
//! `data/record-map.php`).

use std::path::Path;

use serde_json::Value;
use speedyspec_wp_token_map::{CaseSensitivity, MAX_LENGTH, TokenMap, TooLong};

fn unhex(v: &Value) -> Vec<u8> {
    let s = v.as_str().unwrap_or("");
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn recorded() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/map-recorded.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn mode(s: &str) -> CaseSensitivity {
    CaseSensitivity::from_php(s.as_bytes())
}

fn build(case: &Value) -> TokenMap {
    let words: Vec<(Vec<u8>, Vec<u8>)> = case["words"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| (unhex(&p[0]), unhex(&p[1])))
        .collect();
    TokenMap::from_array(&words, case["key_length"].as_u64().unwrap() as usize)
        .expect("short enough")
}

#[test]
fn matches_recorded_wordpress_lookups() {
    for (name, case) in recorded().as_object().unwrap() {
        let map = build(case);
        let view = map.view();
        for read in case["reads"].as_array().unwrap() {
            let text = unhex(&read[0]);
            let offset = read[1].as_u64().unwrap() as usize;
            let got = view.read_token(&text, offset, mode(read[2].as_str().unwrap()));
            let want =
                (!read[3].is_null()).then(|| (unhex(&read[3]), read[4].as_u64().unwrap() as usize));
            assert_eq!(
                got.map(|(m, l)| (m.to_vec(), l)),
                want,
                "{name}: read_token({:?}, {offset}, {})",
                String::from_utf8_lossy(&text),
                read[2]
            );
        }
        for c in case["contains"].as_array().unwrap() {
            let word = unhex(&c[0]);
            assert_eq!(
                view.contains(&word, mode(c[1].as_str().unwrap())),
                c[2].as_bool().unwrap(),
                "{name}: contains({:?}, {})",
                String::from_utf8_lossy(&word),
                c[1]
            );
        }
        let want: Vec<(Vec<u8>, Vec<u8>)> = case["to_array"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| (unhex(&p[0]), unhex(&p[1])))
            .collect();
        assert_eq!(view.to_array(), want, "{name}: to_array");
    }
}

#[test]
fn case_insensitive_small_tokens_keep_wordpress_quirk() {
    // WordPress upper-cases the text, then compares the small word's later bytes case-sensitively (its
    // strtoupper() is applied to a boolean), so an upper-case text matches a lower-case small word only on its first
    // byte.
    let map = TokenMap::from_array(
        &[
            (b"lt".to_vec(), b"<".to_vec()),
            (b"GT".to_vec(), b">".to_vec()),
        ],
        2,
    )
    .unwrap();
    let v = map.view();
    assert_eq!(
        v.read_token(b"lt", 0, CaseSensitivity::AsciiCaseInsensitive),
        None
    );
    assert_eq!(
        v.read_token(b"gt", 0, CaseSensitivity::AsciiCaseInsensitive),
        Some((&b">"[..], 2))
    );
    assert_eq!(
        v.read_token(b"lt", 0, CaseSensitivity::CaseSensitive),
        Some((&b"<"[..], 2))
    );
}

#[test]
fn nul_in_key_stops_the_search() {
    let map = TokenMap::from_array(&[(b"abc".to_vec(), b"1".to_vec())], 2).unwrap();
    let v = map.view();
    assert_eq!(
        v.read_token(b"a\0bc", 0, CaseSensitivity::CaseSensitive),
        None
    );
    assert!(!v.contains(b"ab\0", CaseSensitivity::CaseSensitive));
    assert_eq!(
        v.read_token(b"abcd", 0, CaseSensitivity::CaseSensitive),
        Some((&b"1"[..], 3))
    );
}

#[test]
fn too_long_word_is_rejected() {
    let long = vec![b'a'; MAX_LENGTH];
    assert_eq!(
        TokenMap::from_array(&[(long.clone(), b"x".to_vec())], 2).err(),
        Some(TooLong)
    );
    assert_eq!(
        TokenMap::from_array(&[(b"a".to_vec(), long)], 2).err(),
        Some(TooLong)
    );
    assert!(
        TokenMap::from_array(
            &[(vec![b'a'; MAX_LENGTH - 1], vec![b'x'; MAX_LENGTH - 1])],
            2
        )
        .is_ok()
    );
}

#[test]
fn to_array_round_trips() {
    let words = vec![
        (b"cat".to_vec(), "🐈".as_bytes().to_vec()),
        (b"dog".to_vec(), "🐶".as_bytes().to_vec()),
    ];
    let map = TokenMap::from_array(&words, 2).unwrap();
    let back = TokenMap::from_array(&map.view().to_array(), 2).unwrap();
    assert_eq!(back.view().to_array(), map.view().to_array());
    assert_eq!(
        map.view()
            .read_token(b"cats", 0, CaseSensitivity::CaseSensitive),
        Some(("🐈".as_bytes(), 3))
    );
}

#[test]
fn words_list_their_strings() {
    let map = TokenMap::from_array(
        &[
            (b"lt".to_vec(), b"<".to_vec()),
            (b"abc".to_vec(), b"1".to_vec()),
        ],
        2,
    )
    .unwrap();
    let v = map.view();
    assert_eq!(v.small_mappings.slices(), vec![&b"<"[..]]);
    assert_eq!(v.large_words.slices(), vec![&b"\x01c\x011"[..]]);
}
