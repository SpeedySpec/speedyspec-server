//! `TranslationFile`: lookups, PHP array order for duplicates, and the lazily compiled plural rule.

use std::path::PathBuf;
use std::time::Instant;

use speedyspec_wp_i18n::TranslationFile;
use speedyspec_wp_i18n_contracts::{FileIdentity, RawEntry, RawTranslations, ReadError};

fn identity() -> FileIdentity {
    FileIdentity {
        path: PathBuf::from("/x.mo"),
        dev: 1,
        ino: 2,
        size: 3,
        mtime_ns: 4,
    }
}

fn raw(headers: &[(&str, &str)], entries: &[(&[u8], &[u8])]) -> RawTranslations {
    let mut bytes = Vec::new();
    let mut out = Vec::new();
    for (k, v) in entries {
        let ks = bytes.len() as u32;
        bytes.extend_from_slice(k);
        let vs = bytes.len() as u32;
        bytes.extend_from_slice(v);
        out.push(RawEntry {
            key: ks..vs,
            value: vs..bytes.len() as u32,
        });
    }
    RawTranslations {
        bytes,
        headers: headers
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect(),
        entries: out,
    }
}

#[test]
fn lookups_return_whole_value_first_form_and_forms() {
    let f = TranslationFile::new(identity(), raw(&[], &[(b"ctx\x04one", b"eins\0zwei")]));
    assert_eq!(f.translate(b"ctx\x04one"), Some(&b"eins\0zwei"[..]));
    assert_eq!(f.first_form(b"ctx\x04one"), Some(&b"eins"[..]));
    assert_eq!(
        f.forms(b"ctx\x04one").unwrap().collect::<Vec<_>>(),
        vec![&b"eins"[..], b"zwei"]
    );
    assert_eq!(f.translate(b"one"), None);
    assert!(f.error().is_none());
}

#[test]
fn duplicate_keeps_first_position_last_value() {
    let f = TranslationFile::new(
        identity(),
        raw(&[], &[(b"a", b"1"), (b"b", b"2"), (b"a", b"3")]),
    );
    let entries: Vec<_> = f.entries().collect();
    assert_eq!(
        entries,
        vec![(&b"a"[..], &b"3"[..]), (&b"b"[..], &b"2"[..])]
    );
    assert_eq!(f.translate(b"a"), Some(&b"3"[..]));
}

#[test]
fn plural_rule_from_header_is_compiled_once() {
    let f = TranslationFile::new(
        identity(),
        raw(
            &[(
                "plural-forms",
                "nplurals=3; plural=(n==1 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2);",
            )],
            &[],
        ),
    );
    assert_eq!(
        (f.plural_form(1), f.plural_form(22), f.plural_form(5)),
        (Ok(0), Ok(1), Ok(2))
    );
}

#[test]
fn no_header_is_english() {
    let f = TranslationFile::new(identity(), raw(&[], &[]));
    assert_eq!(
        (f.plural_form(1), f.plural_form(0), f.plural_form(2)),
        (Ok(0), Ok(1), Ok(1))
    );
}

#[test]
fn failed_file_reports_its_error() {
    let f = TranslationFile::failed(identity(), ReadError::Invalid("Invalid data"));
    assert_eq!(f.error(), Some(&ReadError::Invalid("Invalid data")));
    assert_eq!(f.translate(b"a"), None);
    assert_eq!(f.identity(), &identity());
}

#[test]
#[cfg_attr(debug_assertions, ignore = "timing needs a release build")]
fn index_build_is_fast() {
    let mut seed = 42u64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let keys: Vec<Vec<u8>> = (0..15_000)
        .map(|i| format!("Original string {i} {}", next() % 100_000).into_bytes())
        .collect();
    let entries: Vec<(&[u8], &[u8])> = keys
        .iter()
        .map(|k| (k.as_slice(), &b"Translated string"[..]))
        .collect();
    let r = raw(&[], &entries);
    let start = Instant::now();
    let f = TranslationFile::new(identity(), r);
    let took = start.elapsed();
    assert!(f.translate(&keys[777]).is_some());
    assert!(took.as_micros() < 2_000, "built in {took:?}");
}
