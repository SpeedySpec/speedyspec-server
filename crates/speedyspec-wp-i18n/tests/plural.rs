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

#[test]
fn non_ascii_symbol_is_an_error_not_a_panic() {
    let e = PluralRule::compile("n\u{a0}!= 1").unwrap_err();
    assert!(e.to_string().starts_with("Unknown symbol"), "{e}");
    let rule = PluralRule::from_header("nplurals=2; plural=n\u{a0}!= 1;");
    assert_eq!((rule.eval(1), rule.eval(2)), (Ok(0), Ok(1)));
}

#[test]
fn tokens_are_wordpress_postfix_list() {
    use speedyspec_wp_i18n::PluralToken as T;
    let rule = speedyspec_wp_i18n::PluralRule::compile("n != 1").unwrap();
    assert_eq!(rule.tokens(), vec![T::Var, T::Value(1), T::Op("!=".into())]);
    let ternary = speedyspec_wp_i18n::PluralRule::compile("n ? 1 : 2").unwrap();
    assert_eq!(
        ternary.tokens(),
        vec![T::Var, T::Value(1), T::Value(2), T::Op("?:".into())]
    );
}

#[test]
fn rules_rebuilt_from_tokens_evaluate_alike() {
    use speedyspec_wp_i18n::{PluralError, PluralRule, PluralToken as T};
    let expression =
        "n%10==1 && n%100!=11 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2";
    let rule = PluralRule::compile(expression).unwrap();
    let rebuilt = PluralRule::from_tokens(&rule.tokens());
    for n in -3..200 {
        assert_eq!(rule.eval(n), rebuilt.eval(n), "n = {n}");
    }
    let unknown = PluralRule::from_tokens(&[T::Var, T::Value(1), T::Op("^".into())]);
    assert_eq!(
        unknown.eval(3),
        Err(PluralError::Exception("Unknown operator \"^\"".into()))
    );
    let lone_mod = PluralRule::from_tokens(&[T::Op("%".into())]);
    assert_eq!(lone_mod.eval(3), Err(PluralError::DivisionByZero));
}

#[test]
fn an_open_paren_before_else_is_output_as_wordpress_does() {
    use speedyspec_wp_i18n::PluralToken as T;
    // WordPress's ':' pops everything up to the '?', an open parenthesis included, into the output.
    let rule = PluralRule::compile("n ? (1 : 2").unwrap();
    assert_eq!(
        rule.tokens(),
        vec![
            T::Var,
            T::Value(1),
            T::Op("(".into()),
            T::Value(2),
            T::Op("?:".into())
        ]
    );
    assert_eq!(
        rule.eval(1),
        Err(PluralError::Exception("Unknown operator \"(\"".into()))
    );
}
