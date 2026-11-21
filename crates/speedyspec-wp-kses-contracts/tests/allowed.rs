use indexmap::IndexMap;
use speedyspec_wp_kses_contracts::{
    AllowedAttrs, AllowedHtml, AttrRule, AttrRules, Context, RuleValue,
};

#[test]
fn required_only_when_true() {
    let yes = AttrRules::Rules(vec![AttrRule::Other(
        b"required".to_vec().into(),
        RuleValue::Bool(true),
    )]);
    let one = AttrRules::Rules(vec![AttrRule::Other(
        b"required".to_vec().into(),
        RuleValue::Int(1),
    )]);
    assert!(yes.is_required());
    assert!(!one.is_required());
    assert!(!AttrRules::Allowed.is_required());
}

#[test]
fn context_round_trips() {
    assert_eq!(Context::from_bytes(b"post"), Context::Post);
    assert_eq!(
        Context::from_bytes(b"pre_user_description"),
        Context::PreUserDescription
    );
    assert_eq!(Context::from_bytes(b"x").as_bytes(), b"x");
    assert_eq!(Context::UserDescription.as_bytes(), b"user_description");
}

#[test]
fn lookup_is_by_lowercase_key() {
    let mut tags = IndexMap::new();
    tags.insert(b"a".to_vec().into_boxed_slice(), AllowedAttrs::Empty);
    let allowed = AllowedHtml { tags };
    assert!(allowed.get(b"a").is_some());
    assert!(allowed.get(b"A").is_none());
}
