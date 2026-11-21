//! `translate_settings_using_i18n_schema()`'s rules over the domain's own settings value.

use speedyspec_hooks::Key;
use speedyspec_wp_i18n::{I18nError, SchemaValue, translate_settings};

fn st(v: &str) -> SchemaValue {
    SchemaValue::Str(v.as_bytes().to_vec())
}

fn obj(items: &[(&str, SchemaValue)]) -> SchemaValue {
    SchemaValue::Object(
        items
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect(),
    )
}

fn list(items: Vec<SchemaValue>) -> SchemaValue {
    SchemaValue::List(
        items
            .into_iter()
            .enumerate()
            .map(|(i, v)| (Key::Int(i as i64), v))
            .collect(),
    )
}

/// Marks each translation `text|context|domain` so the call is visible.
fn run(schema: &SchemaValue, settings: SchemaValue) -> SchemaValue {
    translate_settings(
        schema,
        settings,
        &st("dom"),
        &mut |text, context, domain| {
            let domain = match domain {
                SchemaValue::Str(d) => String::from_utf8_lossy(d).into_owned(),
                _ => "?".into(),
            };
            Ok::<_, I18nError>(st(&format!(
                "{}|{}|{domain}",
                String::from_utf8_lossy(text),
                String::from_utf8_lossy(context)
            )))
        },
    )
    .unwrap()
}

#[test]
fn string_schema_translates_with_context() {
    assert_eq!(
        run(&st("block title"), st("Notice")),
        st("Notice|block title|dom")
    );
}

#[test]
fn empty_schema_settings_or_domain_return_settings() {
    assert_eq!(run(&st("0"), st("x")), st("x"));
    assert_eq!(run(&st(""), st("x")), st("x"));
    assert_eq!(run(&st("ctx"), st("")), st(""));
    let out = translate_settings(&st("ctx"), st("x"), &st("0"), &mut |_, _, _| {
        Ok(st("never"))
    })
    .unwrap();
    assert_eq!(out, st("x"));
}

#[test]
fn list_schema_renumbers() {
    let settings = SchemaValue::List(vec![
        (Key::Str("k".into()), st("a")),
        (Key::Int(5), st("b")),
    ]);
    assert_eq!(
        run(&list(vec![st("kw")]), settings),
        list(vec![st("a|kw|dom"), st("b|kw|dom")])
    );
}

#[test]
fn list_schema_without_index_zero_keeps_values() {
    let schema = SchemaValue::List(vec![(Key::Int(1), st("kw"))]);
    assert_eq!(run(&schema, list(vec![st("a")])), list(vec![st("a")]));
}

#[test]
fn object_schema_uses_star_fallback_and_keeps_unlisted() {
    let schema = obj(&[("a", st("ctx")), ("*", st("star"))]);
    let settings = SchemaValue::List(vec![
        (Key::Str("a".into()), st("x")),
        (Key::Str("b".into()), st("y")),
        (Key::Int(0), st("z")),
    ]);
    assert_eq!(
        run(&schema, settings),
        SchemaValue::List(vec![
            (Key::Str("a".into()), st("x|ctx|dom")),
            (Key::Str("b".into()), st("y|star|dom")),
            (Key::Int(0), st("z|star|dom")),
        ])
    );
    let plain = obj(&[("a", st("ctx"))]);
    let kept = SchemaValue::List(vec![(
        Key::Str("o".into()),
        SchemaValue::Other(speedyspec_hooks::Value::Int(3)),
    )]);
    assert_eq!(run(&plain, kept.clone()), kept);
}

#[test]
fn null_property_is_not_set() {
    let schema = obj(&[("a", SchemaValue::Null)]);
    let settings = SchemaValue::List(vec![(Key::Str("a".into()), st("x"))]);
    assert_eq!(run(&schema, settings.clone()), settings);
}

#[test]
fn mismatched_kinds_return_settings() {
    assert_eq!(run(&st("ctx"), list(vec![st("a")])), list(vec![st("a")]));
    assert_eq!(run(&list(vec![st("ctx")]), st("x")), st("x"));
    assert_eq!(
        run(
            &SchemaValue::Other(speedyspec_hooks::Value::Int(5)),
            st("x")
        ),
        st("x")
    );
}
