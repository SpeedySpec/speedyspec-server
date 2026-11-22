//! Operations and types shared by the REST tests.
#![allow(dead_code)]

use std::rc::Rc;

use speedyspec_operation::{Operation, OperationBuilder, Registries, S, Type};
use speedyspec_operation_contracts::{Action, RawInput, RawOutput};
use speedyspec_rest::{NoRestHooks, RestTransport, RouteTable};
use speedyspec_transport::{Exposure, MemoryBindingStore, TransportRegistry};

pub fn op(
    builder: OperationBuilder,
    input: speedyspec_operation::SchemaBuilder,
) -> OperationBuilder {
    builder
        .input::<RawInput>(input)
        .output::<RawOutput>(S::any())
        .public()
}

pub fn id() -> speedyspec_operation::SchemaBuilder {
    S::object([("id", S::integer().required())])
}

pub fn none() -> speedyspec_operation::SchemaBuilder {
    S::object([])
}

/// Types `report` (integer id) and `category` (string slug), and one operation per default-route case.
pub fn registries() -> Rc<Registries> {
    let r = Registries::new();
    r.register_type(Type::resource("report").field("id", S::integer()).build())
        .unwrap();
    r.register_type(
        Type::resource("category")
            .field("slug", S::string())
            .build(),
    )
    .unwrap();
    let ops = [
        op(
            Operation::query("acme/list-reports").resource("report", Action::List, &[]),
            none(),
        ),
        op(
            Operation::query("acme/get-report").resource("report", Action::Get, &["id"]),
            id(),
        ),
        op(
            Operation::command("acme/create-report").resource("report", Action::Create, &[]),
            none(),
        ),
        op(
            Operation::command("acme/update-report").resource("report", Action::Update, &["id"]),
            id(),
        ),
        op(
            Operation::command("acme/delete-report").resource("report", Action::Delete, &["id"]),
            id(),
        ),
        op(
            Operation::command("acme/archive-report").resource(
                "report",
                Action::Custom("archive".into()),
                &["id"],
            ),
            id(),
        ),
        op(
            Operation::query("acme/preview-report").resource(
                "report",
                Action::Custom("preview".into()),
                &["id"],
            ),
            id(),
        ),
        op(Operation::query("acme/ping"), none()),
        op(Operation::command("acme/do-thing"), none()),
        op(
            Operation::query("acme/list-categories").resource("category", Action::List, &[]),
            none(),
        ),
        op(
            Operation::query("acme/get-category").resource("category", Action::Get, &["slug"]),
            S::object([("slug", S::string().required())]),
        ),
        op(Operation::subscription("acme/watch-reports"), none()),
    ];
    for builder in ops {
        r.register_operation(builder.build().unwrap()).unwrap();
    }
    r.close().unwrap();
    Rc::new(r)
}

pub fn exposure(registries: &Rc<Registries>, store: Rc<MemoryBindingStore>) -> Exposure {
    let transports = TransportRegistry::new(store);
    transports
        .install(Rc::new(RestTransport::new(registries.clone())))
        .unwrap();
    transports.expose(registries).unwrap()
}

pub fn table(
    registries: &Rc<Registries>,
    store: Rc<MemoryBindingStore>,
) -> (RouteTable, Vec<String>, Exposure) {
    let exposure = exposure(registries, store);
    let (table, problems) = RouteTable::build(&exposure, &NoRestHooks);
    (table, problems, exposure)
}
