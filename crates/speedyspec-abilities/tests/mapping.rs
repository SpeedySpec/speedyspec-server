use speedyspec_abilities::{ABILITIES, AbilitiesTransport, DefaultAbilityMapping, category_slug};
use speedyspec_abilities_contracts::{AbilityBinding, AbilityMapping, Annotations};
use speedyspec_operation::{Operation, OperationBuilder, S};
use speedyspec_operation_contracts::{Action, OperationSpec, RawInput, RawOutput, Value};
use speedyspec_transport_contracts::Transport;

fn spec(builder: OperationBuilder) -> OperationSpec {
    builder
        .input::<RawInput>(S::object([("id", S::integer())]))
        .output::<RawOutput>(S::any())
        .public()
        .build()
        .unwrap()
}

fn annotations(readonly: bool, destructive: bool, idempotent: bool) -> Annotations {
    Annotations {
        readonly: Some(readonly),
        destructive: Some(destructive),
        idempotent: Some(idempotent),
    }
}

fn binding(builder: OperationBuilder) -> Option<AbilityBinding> {
    DefaultAbilityMapping.binding(&spec(builder))
}

#[test]
fn a_query_is_readonly() {
    let b = binding(
        Operation::query("acme/get-report")
            .resource("report", Action::Get, &["id"])
            .label("Get report", "Gets one report."),
    )
    .unwrap();
    assert_eq!(b.annotations, annotations(true, false, true));
    assert_eq!(b.category.as_deref(), Some("acme"));
    assert_eq!(b.show_in_rest, Some(true));
    assert_eq!(b.public, Some(false));
    assert_eq!(b.label.as_deref(), Some("Get report"));
    assert_eq!(b.description.as_deref(), Some("Gets one report."));
}

#[test]
fn unlabelled_operations_use_their_name() {
    let b = binding(Operation::query("acme/ping")).unwrap();
    assert_eq!(b.label.as_deref(), Some("acme/ping"));
    assert_eq!(b.description.as_deref(), Some("acme/ping"));
}

#[test]
fn a_delete_command_is_destructive_and_idempotent() {
    let b = binding(Operation::command("acme/delete-report").resource(
        "report",
        Action::Delete,
        &["id"],
    ))
    .unwrap();
    assert_eq!(b.annotations, annotations(false, true, true));
}

#[test]
fn an_update_command_is_idempotent_not_destructive() {
    let b = binding(Operation::command("acme/update-report").resource(
        "report",
        Action::Update,
        &["id"],
    ))
    .unwrap();
    assert_eq!(b.annotations, annotations(false, false, true));
}

#[test]
fn a_plain_command_takes_the_operations_traits() {
    let b = binding(Operation::command("acme/do-thing")).unwrap();
    assert_eq!(b.annotations, annotations(false, false, false));
    let b = binding(Operation::command("acme/wipe").destructive().idempotent()).unwrap();
    assert_eq!(b.annotations, annotations(false, true, true));
}

#[test]
fn a_vendor_with_odd_characters_becomes_a_slug() {
    assert_eq!(category_slug("my_Plugin"), "my-plugin");
    assert_eq!(category_slug("--a..b--"), "a-b");
    assert_eq!(category_slug("acme"), "acme");
}

#[test]
fn subscriptions_have_no_ability() {
    assert_eq!(
        binding(Operation::subscription("acme/report-changed")),
        None
    );
}

#[test]
fn a_dashy_vendor_still_gets_a_valid_category() {
    let b = binding(Operation::query("-my--plugin-/x")).unwrap();
    assert_eq!(b.category.as_deref(), Some("my-plugin"));
}

#[test]
fn the_transport_uses_the_mapping_and_checks_bindings() {
    let transport = AbilitiesTransport::new();
    assert_eq!(transport.name().as_str(), ABILITIES);
    let report = spec(Operation::query("acme/get-report"));
    let value = transport.default_binding(&report).unwrap();
    assert_eq!(value.get("category"), Some(&Value::from("acme")));
    let explicit = Value::map([("category", Value::from("reports"))]);
    assert!(transport.check_binding(&report, &explicit).is_ok());
    let bad = Value::map([("category", Value::from("Reports"))]);
    assert!(transport.check_binding(&report, &bad).is_err());
}
