//! Replays what `compat/text-diff/record.php` recorded from WordPress 7.1.5 on PHP 8.5.12.
//!
//! Every recorded value must come out byte for byte: escaping, word splitting, PHP's loose `==`, the string
//! distance, the engine's op scripts, the inline word diffs, and `wp_text_diff()` itself (cases whose arguments the
//! PHP facade handles, query strings, objects and filters, are replayed through PHP in the PHP crate).

use base64::Engine as _;
use serde_json::Value;
use speedyspec_wp_text_diff::contracts::{
    DiffOp, NoLineFilter, RowLabels, SequenceDiffer, TableOptions, Title,
};
use speedyspec_wp_text_diff::{
    HordeSequenceDiffer, TextDiff, TextDiffError, byte_counts, html_special_chars, inline_diff,
    php_loose_eq, split_words, string_distance,
};

fn fixture(name: &str) -> Value {
    let path = format!(
        "{}/../../compat/text-diff/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}")))
        .unwrap()
}

fn bytes(v: &Value) -> Vec<u8> {
    match v {
        Value::String(s) => s.as_bytes().to_vec(),
        Value::Object(o) => base64::engine::general_purpose::STANDARD
            .decode(o["b64"].as_str().unwrap())
            .unwrap(),
        other => panic!("not a string: {other}"),
    }
}

fn show(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

#[test]
fn escaping() {
    let units = fixture("units.json");
    for pair in units["escape"].as_array().unwrap() {
        let (input, expected) = (bytes(&pair[0]), bytes(&pair[1]));
        assert_eq!(
            html_special_chars(&input).as_ref(),
            expected.as_slice(),
            "input {:?}",
            show(&input)
        );
    }
}

#[test]
fn word_splitting() {
    let units = fixture("units.json");
    for pair in units["words"].as_array().unwrap() {
        let input = bytes(&pair[0]);
        let got: Option<Vec<Vec<u8>>> =
            split_words(&input).map(|w| w.pieces().map(|p| p.to_vec()).collect());
        let expected: Option<Vec<Vec<u8>>> =
            pair[1].as_array().map(|a| a.iter().map(bytes).collect());
        assert_eq!(got, expected, "input {:?}", show(&input));
    }
}

#[test]
fn loose_equality() {
    let units = fixture("units.json");
    for t in units["loose"].as_array().unwrap() {
        let (a, b) = (t[0].as_str().unwrap(), t[1].as_str().unwrap());
        assert_eq!(
            php_loose_eq(a.as_bytes(), b.as_bytes()),
            t[2].as_bool().unwrap(),
            "{a:?} == {b:?}"
        );
    }
}

#[test]
fn string_distances() {
    let units = fixture("units.json");
    for t in units["distance"].as_array().unwrap() {
        let (a, b) = (bytes(&t[0]), bytes(&t[1]));
        let got = string_distance(&a, &byte_counts(&a), &byte_counts(&b));
        assert_eq!(
            got,
            t[2].as_f64().unwrap(),
            "{:?} vs {:?}",
            show(&a),
            show(&b)
        );
    }
}

fn script(ops: &[DiffOp]) -> Vec<(String, usize, usize)> {
    ops.iter()
        .map(|op| match op {
            DiffOp::Copy { orig, .. } => ("copy".into(), orig.len(), orig.len()),
            DiffOp::Delete { orig } => ("delete".into(), orig.len(), 0),
            DiffOp::Add { fin } => ("add".into(), 0, fin.len()),
            DiffOp::Change { orig, fin } => ("change".into(), orig.len(), fin.len()),
        })
        .collect()
}

#[test]
fn engine_op_scripts() {
    let cases = fixture("engine.json");
    let mut failures = Vec::new();
    for (i, c) in cases.as_array().unwrap().iter().enumerate() {
        let a: Vec<Vec<u8>> = c["orig"].as_array().unwrap().iter().map(bytes).collect();
        let b: Vec<Vec<u8>> = c["final"].as_array().unwrap().iter().map(bytes).collect();
        let a: Vec<&[u8]> = a.iter().map(|v| v.as_slice()).collect();
        let b: Vec<&[u8]> = b.iter().map(|v| v.as_slice()).collect();
        let expected: Vec<(String, usize, usize)> = c["ops"]
            .as_array()
            .unwrap()
            .iter()
            .map(|o| {
                (
                    o[0].as_str().unwrap().into(),
                    o[1].as_u64().unwrap() as usize,
                    o[2].as_u64().unwrap() as usize,
                )
            })
            .collect();
        let got = script(&HordeSequenceDiffer.diff(&a, &b));
        if got != expected {
            failures.push(format!(
                "case {i}: {a:?} -> {b:?}\n  got      {got:?}\n  expected {expected:?}",
                a = a.iter().map(|x| show(x)).collect::<Vec<_>>(),
                b = b.iter().map(|x| show(x)).collect::<Vec<_>>()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} scripts differ:\n{}",
        failures.len(),
        cases.as_array().unwrap().len(),
        failures
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn inline_word_diffs() {
    let cases = fixture("inline.json");
    let mut failures = Vec::new();
    for c in cases.as_array().unwrap() {
        let (a, b, expected) = (bytes(&c[0]), bytes(&c[1]), bytes(&c[2]));
        let got = inline_diff(&a, &b, &HordeSequenceDiffer).unwrap().render();
        if got != expected {
            failures.push(format!(
                "{:?} -> {:?}\n  got      {:?}\n  expected {:?}",
                show(&a),
                show(&b),
                show(&got),
                show(&expected)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// `normalize_whitespace()`, for this test only: the port gets it from the text formatting port.
fn normalize_whitespace(s: &[u8]) -> Vec<u8> {
    let trim = |b: &u8| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0 | 0x0B);
    let start = s.iter().position(|b| !trim(b)).unwrap_or(s.len());
    let end = s.iter().rposition(|b| !trim(b)).map_or(start, |e| e + 1);
    let mut out = Vec::new();
    for &b in &s[start..end] {
        let b = if b == b'\r' { b'\n' } else { b };
        match (b, out.last()) {
            (b'\n', Some(b'\n')) => {}
            (b' ' | b'\t', Some(b' ')) => {}
            (b'\t', _) => out.push(b' '),
            _ => out.push(b),
        }
    }
    out
}

/// What the facade passes for an array argument (or `null`), as PHP evaluates it.
fn options(args: &Value) -> Option<TableOptions> {
    let mut o = TableOptions::default();
    match args {
        Value::Null => {}
        Value::Object(map) => {
            for (k, v) in map {
                let (text, truthy) = match v {
                    Value::String(s) => (s.clone(), !(s.is_empty() || s == "0")),
                    Value::Bool(b) => (if *b { "1".into() } else { String::new() }, *b),
                    Value::Number(n) => (n.to_string(), n.as_f64() != Some(0.0)),
                    _ => return None,
                };
                let title = Title {
                    text: text.into_bytes(),
                    truthy,
                };
                match k.as_str() {
                    "title" => o.title = title,
                    "title_left" => o.title_left = title,
                    "title_right" => o.title_right = title,
                    "show_split_view" => o.split_view = truthy,
                    _ => {}
                }
            }
        }
        _ => return None,
    }
    Some(o)
}

#[test]
fn wp_text_diff_cases() {
    let cases = fixture("cases.json");
    let diff = TextDiff::default();
    let mut failures = Vec::new();
    let mut replayed = 0;
    for c in cases.as_array().unwrap() {
        if c.get("filter").is_some() {
            continue;
        }
        let Some(options) = options(&c["args"]) else {
            continue;
        };
        let (l, r) = (
            normalize_whitespace(&bytes(&c["left"])),
            normalize_whitespace(&bytes(&c["right"])),
        );
        let got = diff.table(&l, &r, &options, &RowLabels::default(), &NoLineFilter);
        replayed += 1;
        let name = c["name"].as_str().unwrap();
        match (c.get("throws"), got) {
            (Some(_), Err(TextDiffError::InvalidUtf8)) => {}
            (None, Ok(table)) => {
                let expected = bytes(&c["output"]);
                let got = table.unwrap_or_default();
                if got != expected {
                    failures.push(format!(
                        "{name}:\n  got      {:?}\n  expected {:?}",
                        show(&got),
                        show(&expected)
                    ));
                }
            }
            (t, g) => failures.push(format!("{name}: throws {t:?}, got {g:?}")),
        }
    }
    assert!(replayed > 400, "only {replayed} cases replayed");
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}
