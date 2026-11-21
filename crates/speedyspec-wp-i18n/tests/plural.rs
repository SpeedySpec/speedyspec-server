//! Plural-Forms expressions, against WordPress 7.1.5's `Plural_Forms` (recorded in `data/plural-forms.json` by
//! `data/record-plural-forms.php` over every GlotPress locale and edge cases).

use speedyspec_wp_i18n::{PluralError, PluralRule};

fn php_result(r: Result<i64, PluralError>) -> serde_json::Value {
    match r {
        Ok(n) => serde_json::json!(n),
        Err(e) => serde_json::json!(format!("{}: {}", e.php_class(), e)),
    }
}

#[test]
fn every_recorded_expression_matches_php() {
    let recorded: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("data/plural-forms.json")).expect("plural-forms.json");
    assert!(recorded.len() > 40);
    for entry in recorded {
        let expression = entry["expression"].as_str().unwrap();
        match PluralRule::compile(expression) {
            Err(e) => assert_eq!(
                Some(format!("{}: {}", e.php_class(), e).as_str()),
                entry["error"].as_str(),
                "{expression:?} compiled with an error"
            ),
            Ok(rule) => {
                assert!(
                    entry["error"].is_null(),
                    "{expression:?} should fail: {}",
                    entry["error"]
                );
                for pair in entry["values"].as_array().unwrap() {
                    let n = pair[0].as_i64().unwrap();
                    assert_eq!(php_result(rule.eval(n)), pair[1], "{expression:?} n={n}");
                }
            }
        }
    }
}

#[test]
fn strict_equality_between_bool_and_int() {
    assert_eq!(PluralRule::compile("(n > 1) == 1").unwrap().eval(5), Ok(0));
}

#[test]
fn modulo_by_zero_errors() {
    assert_eq!(
        PluralRule::compile("n % 0").unwrap().eval(3),
        Err(PluralError::DivisionByZero)
    );
}

#[test]
fn bad_header_falls_back_to_english() {
    let rule = PluralRule::from_header("nplurals=2; plural=n !! 1;");
    assert_eq!((rule.eval(1), rule.eval(2)), (Ok(0), Ok(1)));
}

#[test]
fn header_expression_is_extracted() {
    let rule = PluralRule::from_header(
        "nplurals=3; plural=(n==1 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2);",
    );
    assert_eq!(
        (rule.eval(1), rule.eval(22), rule.eval(5)),
        (Ok(0), Ok(1), Ok(2))
    );
    // No space after the semicolon: WordPress's pattern needs one, so this is English.
    let rule = PluralRule::from_header("nplurals=3;plural=n==1 ? 0 : 2");
    assert_eq!(rule.eval(5), Ok(1));
}
