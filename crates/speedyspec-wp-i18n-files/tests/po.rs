//! The PO reader and writer, against WordPress 7.1.5's `PO` (recorded in `data/po-recorded.json` by
//! `data/record-po.php`).

use std::path::{Path, PathBuf};

use serde_json::Value;
use speedyspec_wp_i18n_contracts::PoEntry;
use speedyspec_wp_i18n_files::po;

fn data() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data")
}

fn recorded() -> Value {
    serde_json::from_str(&std::fs::read_to_string(data().join("po-recorded.json")).unwrap())
        .unwrap()
}

fn unhex(v: &Value) -> Vec<u8> {
    let s = v.as_str().unwrap_or("");
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn opt(v: &Value) -> Option<Vec<u8>> {
    (!v.is_null()).then(|| unhex(v))
}

fn list(v: &Value) -> Vec<Vec<u8>> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|pair| unhex(&pair[1]))
        .collect()
}

fn entry(v: &Value) -> PoEntry {
    PoEntry {
        context: opt(&v["context"]),
        singular: opt(&v["singular"]),
        plural: opt(&v["plural"]),
        translations: v["translations"]
            .as_array()
            .unwrap()
            .iter()
            .map(|pair| {
                let key = match pair[0].as_i64() {
                    Some(n) => speedyspec_wp_i18n_contracts::PhpArrayKey::Int(n),
                    None => speedyspec_wp_i18n_contracts::PhpArrayKey::Str(
                        pair[0].as_str().unwrap().to_owned(),
                    ),
                };
                (key, unhex(&pair[1]))
            })
            .collect(),
        is_plural: v["is_plural"].as_bool().unwrap(),
        translator_comments: unhex(&v["translator_comments"]),
        extracted_comments: unhex(&v["extracted_comments"]),
        references: list(&v["references"]),
        flags: list(&v["flags"]),
    }
}

fn file(name: &str) -> PathBuf {
    match name.split_once('/') {
        Some(("po", f)) => data().join("po").join(f),
        Some(("pomo", f)) => data().join("../../../../tests/php/data/pomo").join(f),
        _ => panic!("{name}"),
    }
}

#[test]
fn parses_every_recorded_po() {
    let rec = recorded();
    for (name, want) in rec["files"].as_object().unwrap() {
        let bytes = std::fs::read(file(name)).unwrap();
        let got = po::parse_po(&bytes);
        let entries: Vec<PoEntry> = want["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(entry)
            .collect();
        assert_eq!(got.entries, entries, "{name}: entries");
        assert_eq!(got.failed, want["ended"] == "false", "{name}: how it ended");
    }
}

#[test]
fn crlf_and_cr_files_parse_like_lf() {
    let lf = po::parse_po(b"msgid \"\"\nmsgstr \"X: y\\n\"\n\nmsgid \"a\"\nmsgstr \"b\"\n");
    let cr = po::parse_po(&std::fs::read(data().join("po/cr.po")).unwrap());
    assert_eq!(lf.entries, cr.entries);
}

#[test]
fn unterminated_quote_fails_the_entry_it_ends() {
    // The bad line is read while the first entry is still open, so WordPress returns false before returning it.
    let got = po::parse_po(b"msgid \"a\"\nmsgstr \"b\"\n\nmsgid \"abc\nmsgstr \"x\"\n");
    assert_eq!(got.entries.len(), 0);
    assert!(got.failed);
    let after = po::parse_po(
        b"msgid \"a\"\nmsgstr \"b\"\n\nmsgid \"c\"\nmsgstr \"d\"\n\nmsgid \"abc\nmsgstr \"x\"\n",
    );
    assert_eq!(after.entries.len(), 1);
    assert!(after.failed);
}

#[test]
fn po_static_helpers_match_wordpress() {
    for call in recorded()["static"].as_array().unwrap() {
        let name = call[0].as_str().unwrap();
        let args: Vec<Vec<u8>> = call[1].as_array().unwrap().iter().map(unhex).collect();
        let want = unhex(&call[2]);
        let got = match name {
            "poify" => po::poify(&args[0]),
            "unpoify" => po::unpoify(&args[0]),
            "prepend_each_line" => po::prepend_each_line(&args[0], &args[1]),
            "comment_block" => po::comment_block(&args[0], args[1][0]),
            "trim_quotes" => po::trim_quotes(&args[0]).to_vec(),
            "match_begin_and_end_newlines" => po::match_begin_and_end_newlines(&args[0], &args[1]),
            other => panic!("{other}"),
        };
        assert_eq!(
            got,
            want,
            "{name}({:?})",
            args.iter()
                .map(|a| String::from_utf8_lossy(a).into_owned())
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn export_entry_matches_wordpress() {
    for case in recorded()["export_entry"].as_array().unwrap() {
        let e = entry(&case[0]);
        let want = (!case[1].is_boolean()).then(|| unhex(&case[1]));
        assert_eq!(po::export_entry(&e), want, "{e:?}");
    }
}
