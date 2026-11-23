use std::rc::Rc;

use speedyspec_operation::{Operation, OperationBuilder, Registries, S, SchemaBuilder, Type};
use speedyspec_operation_contracts::{
    Action, Category, OperationCatalog, OperationSpec, RawInput, RawOutput, Value,
};
use speedyspec_transport_contracts::Transport;
use speedyspec_xmlrpc::{DefaultFaultMapping, DefaultXmlRpcMapping, XMLRPC, XmlRpcTransport};
use speedyspec_xmlrpc_contracts::{FaultMapping, XmlRpcBinding, XmlRpcMapping, XmlRpcParam};

fn op(builder: OperationBuilder, input: SchemaBuilder) -> OperationBuilder {
    builder
        .input::<RawInput>(input)
        .output::<RawOutput>(S::any())
        .public()
}

fn registries() -> Rc<Registries> {
    let r = Registries::new();
    r.register_type(Type::resource("report").field("id", S::integer()).build())
        .unwrap();
    r.register_type(
        Type::resource("post_type")
            .field("slug", S::string())
            .build(),
    )
    .unwrap();
    let id = || S::object([("id", S::integer().required())]);
    let id_and_title = || S::object([("id", S::integer().required()), ("title", S::string())]);
    let filter = || S::object([("status", S::string())]);
    let none = || S::object([]);
    let ops = [
        op(
            Operation::query("acme/list-reports").resource("report", Action::List, &[]),
            filter(),
        ),
        op(
            Operation::query("acme/list-post-types").resource("post_type", Action::List, &[]),
            none(),
        ),
        op(
            Operation::query("acme/get-report").resource("report", Action::Get, &["id"]),
            id(),
        ),
        op(
            Operation::command("acme/create-report").resource("report", Action::Create, &[]),
            filter(),
        ),
        op(
            Operation::command("acme/update-report").resource("report", Action::Update, &["id"]),
            id_and_title(),
        ),
        op(
            Operation::command("acme/delete-report").resource("report", Action::Delete, &["id"]),
            id(),
        ),
        op(
            Operation::command("acme/mark-read-report").resource(
                "report",
                Action::Custom("mark-read".into()),
                &["id"],
            ),
            id(),
        ),
        op(Operation::command("my-plugin/do-thing"), filter()),
        op(Operation::query("acme/ping"), none()),
        op(Operation::subscription("acme/report-changed"), none()),
    ];
    for builder in ops {
        r.register_operation(builder.build().unwrap()).unwrap();
    }
    Rc::new(r)
}

fn spec(registries: &Registries, name: &str) -> Rc<OperationSpec> {
    registries.get_operation(name).expect(name)
}

fn binding(name: &str) -> Option<XmlRpcBinding> {
    let r = registries();
    DefaultXmlRpcMapping.binding(&spec(&r, name), r.as_ref())
}

fn params(names: &[&str]) -> Vec<XmlRpcParam> {
    names.iter().map(|n| XmlRpcParam::parse(n)).collect()
}

fn expect(name: &str, method: &str, names: &[&str]) {
    assert_eq!(
        binding(name),
        Some(XmlRpcBinding {
            method: method.into(),
            params: params(names)
        }),
        "{name}"
    );
}

#[test]
fn list_maps_to_get_plural() {
    expect(
        "acme/list-reports",
        "acme.getReports",
        &["blog_id", "username", "password", "*"],
    );
    expect(
        "acme/list-post-types",
        "acme.getPostTypes",
        &["blog_id", "username", "password"],
    );
}

#[test]
fn get_maps_with_identity() {
    expect(
        "acme/get-report",
        "acme.getReport",
        &["blog_id", "username", "password", "id"],
    );
}

#[test]
fn create_maps_to_new() {
    expect(
        "acme/create-report",
        "acme.newReport",
        &["blog_id", "username", "password", "*"],
    );
}

#[test]
fn update_maps_to_edit() {
    expect(
        "acme/update-report",
        "acme.editReport",
        &["blog_id", "username", "password", "id", "*"],
    );
}

#[test]
fn delete_maps_to_delete() {
    expect(
        "acme/delete-report",
        "acme.deleteReport",
        &["blog_id", "username", "password", "id"],
    );
}

#[test]
fn custom_action_maps_to_camel_case_action() {
    expect(
        "acme/mark-read-report",
        "acme.markReadReport",
        &["blog_id", "username", "password", "id"],
    );
}

#[test]
fn no_resource_maps_to_camel_case() {
    expect(
        "my-plugin/do-thing",
        "my_plugin.doThing",
        &["blog_id", "username", "password", "*"],
    );
    expect(
        "acme/ping",
        "acme.ping",
        &["blog_id", "username", "password"],
    );
}

#[test]
fn subscriptions_have_no_method() {
    assert_eq!(binding("acme/report-changed"), None);
}

#[test]
fn fault_codes_follow_wordpress() {
    let m = DefaultFaultMapping;
    for (category, code) in [
        (Category::InvalidArgument, 400),
        (Category::PermissionDenied, 401),
        (Category::Unauthenticated, 403),
        (Category::NotFound, 404),
        (Category::AlreadyExists, 409),
        (Category::Conflict, 409),
        (Category::ResourceExhausted, 429),
        (Category::Unavailable, 503),
        (Category::Internal, 500),
    ] {
        assert_eq!(m.code(category), code, "{category:?}");
    }
}

#[test]
fn transport_uses_the_mapping() {
    let r = registries();
    let transport = XmlRpcTransport::new(r.clone());
    assert_eq!(transport.name().as_str(), XMLRPC);
    let value = transport
        .default_binding(&spec(&r, "acme/update-report"))
        .unwrap();
    assert_eq!(
        value.get("method"),
        Some(&Value::String("acme.editReport".into()))
    );
}

#[test]
fn check_binding_refuses_unknown_fields_and_misplaced_rest() {
    let r = registries();
    let transport = XmlRpcTransport::new(r.clone());
    let update = spec(&r, "acme/update-report");
    let with = |names: &[&str]| {
        XmlRpcBinding {
            method: "acme.change".into(),
            params: params(names),
        }
        .to_value()
    };
    assert!(
        transport
            .check_binding(&update, &with(&["username", "password", "id", "*"]))
            .is_ok()
    );
    assert!(
        transport
            .check_binding(&update, &with(&["id", "nope"]))
            .is_err()
    );
    assert!(
        transport
            .check_binding(&update, &with(&["*", "id"]))
            .is_err()
    );
    assert!(
        transport
            .check_binding(&update, &with(&["id", "*", "*"]))
            .is_err()
    );
    let bad_name = Value::map([("method", Value::String("system.x".into()))]);
    assert!(transport.check_binding(&update, &bad_name).is_err());
}
