//! The HTML5 named character references: WordPress 7.1.5's precomputed table, and the lookup the HTML/kses port
//! uses.

use std::path::Path;

use speedyspec_wp_token_map::{
    CaseSensitivity, TokenMap, html5, named_reference, named_reference_count,
};

/// `entities.json`'s names (without the leading `&`) and characters.
fn entities() -> Vec<(Vec<u8>, Vec<u8>)> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/php/data/html5-entities/entities.json");
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    json.as_object()
        .unwrap()
        .iter()
        .map(|(name, v)| {
            (
                name.as_bytes()[1..].to_vec(),
                v["characters"].as_str().unwrap().as_bytes().to_vec(),
            )
        })
        .collect()
}

#[test]
fn every_entity_resolves_longest_first() {
    let all = entities();
    assert_eq!(all.len(), 2231);
    assert_eq!(named_reference_count(), 2231);
    for (name, characters) in &all {
        let mut text = name.clone();
        text.push(b'x');
        assert_eq!(
            named_reference(&text, 0),
            Some((characters.as_slice(), name.len())),
            "{}",
            String::from_utf8_lossy(name)
        );
    }
    assert_eq!(named_reference(b"notin;", 0), Some(("∉".as_bytes(), 6)));
    assert_eq!(named_reference(b"notit;", 0), Some(("¬".as_bytes(), 3)));
}

#[test]
fn double_escaped_names_are_not_found() {
    for (name, _) in entities() {
        let mut escaped = b"&amp;".to_vec();
        escaped.extend_from_slice(&name);
        assert!(!html5().contains(&escaped, CaseSensitivity::CaseSensitive));
        assert!(html5().contains(&name, CaseSensitivity::CaseSensitive));
    }
}

#[test]
fn the_table_is_wordpress_from_array() {
    let built = TokenMap::from_array(&entities(), 2).unwrap();
    assert_eq!(html5().to_array(), built.view().to_array());
    let (b, h) = (built.view(), html5());
    assert_eq!(h.groups, b.groups);
    assert_eq!(h.small_words, b.small_words);
    assert_eq!(h.key_length, 2);
}

#[test]
fn kses_cases() {
    assert_eq!(named_reference(b"amp;x", 0), Some((&b"&"[..], 4)));
    assert_eq!(named_reference(b"ampx", 0), Some((&b"&"[..], 3)));
    assert_eq!(named_reference(b"notin;", 0), Some(("∉".as_bytes(), 6)));
    assert_eq!(named_reference(b"zzz", 0), None);
    assert_eq!(named_reference(b"&lt;", 1), Some((&b"<"[..], 3)));
    assert_eq!(named_reference(b"a&", 2), None);
}
