use speedyspec_abilities_contracts::{AbilityBinding, Annotations, is_ability_name};
use speedyspec_operation_contracts::Value;

fn full() -> AbilityBinding {
    AbilityBinding {
        label: Some("Get report".into()),
        description: Some("Gets a report.".into()),
        category: Some("reports".into()),
        show_in_rest: Some(true),
        public: Some(false),
        annotations: Annotations {
            readonly: Some(true),
            destructive: Some(false),
            idempotent: None,
        },
        meta: Value::map([("foo", Value::from("bar"))]),
    }
}

#[test]
fn binding_round_trips_through_value() {
    let binding = full();
    assert_eq!(AbilityBinding::from_value(&binding.to_value()), Ok(binding));
    let empty = AbilityBinding::default();
    assert_eq!(AbilityBinding::from_value(&empty.to_value()), Ok(empty));
}

#[test]
fn binding_refuses_a_bad_category_slug() {
    for slug in ["Reports", "a--b", "-a", "a-", "a_b", ""] {
        let value = Value::map([("category", Value::from(slug))]);
        assert!(AbilityBinding::from_value(&value).is_err(), "{slug}");
    }
    let ok = Value::map([("category", Value::from("site-2"))]);
    assert!(AbilityBinding::from_value(&ok).is_ok());
}

#[test]
fn binding_refuses_unknown_annotations() {
    let value = Value::map([(
        "annotations",
        Value::map([
            ("readonly", Value::Bool(true)),
            ("sparkly", Value::Bool(true)),
        ]),
    )]);
    assert!(AbilityBinding::from_value(&value).is_err());
}

#[test]
fn merge_prefers_explicit_fields() {
    let explicit = AbilityBinding {
        category: Some("reports".into()),
        annotations: Annotations {
            readonly: Some(false),
            ..Annotations::default()
        },
        ..AbilityBinding::default()
    };
    let merged = explicit.merge(full());
    assert_eq!(merged.category.as_deref(), Some("reports"));
    assert_eq!(merged.label.as_deref(), Some("Get report"));
    assert_eq!(merged.annotations.readonly, Some(false));
    assert_eq!(merged.annotations.destructive, Some(false));
    assert_eq!(merged.meta, Value::map([("foo", Value::from("bar"))]));
}

#[test]
fn ability_names_follow_wordpress() {
    assert!(is_ability_name("acme/get-report"));
    assert!(is_ability_name("core/get-site-info"));
    for bad in [
        "Acme/x", "acme_x/y", "a/b/c", "acme", "/x", "x/", "acme/x y",
    ] {
        assert!(!is_ability_name(bad), "{bad}");
    }
}
