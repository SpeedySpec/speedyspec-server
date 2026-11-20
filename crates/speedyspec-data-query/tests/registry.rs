use std::any::Any;
use std::cell::Cell;
use std::collections::BTreeMap;
use std::rc::Rc;

use speedyspec_data::*;
use speedyspec_data_query::EngineRegistry;

struct FakeDirect;
impl DirectStore for FakeDirect {
    fn engine(&self) -> &str {
        "cache"
    }
    fn kind(&self) -> &'static str {
        "fake"
    }
    fn connect(&self) -> Result<(), DbError> {
        Ok(())
    }
    fn health(&self) -> Result<Health, DbError> {
        Ok(Health {
            ok: true,
            detail: String::new(),
        })
    }
    fn close(&self) -> Result<(), DbError> {
        Ok(())
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

struct FakeEngine {
    name: String,
    opens: Rc<Cell<u32>>,
}
impl Engine for FakeEngine {
    fn name(&self) -> &str {
        &self.name
    }
    fn kind(&self) -> EngineKind {
        EngineKind::Memory
    }
    fn open(&self) -> Result<EngineHandle, DbError> {
        self.opens.set(self.opens.get() + 1);
        Ok(EngineHandle::Direct(Rc::new(FakeDirect)))
    }
}

struct FakeFactory {
    opens: Rc<Cell<u32>>,
}
impl EngineFactory for FakeFactory {
    fn scheme(&self) -> &'static str {
        "fake"
    }
    fn create(&self, config: EngineConfig) -> Result<Rc<dyn Engine>, DbError> {
        Ok(Rc::new(FakeEngine {
            name: config.name,
            opens: self.opens.clone(),
        }))
    }
}

fn config(name: &str, url: &str) -> EngineConfig {
    EngineConfig {
        name: name.into(),
        url: url.into(),
        options: BTreeMap::new(),
    }
}

fn registry(opens: &Rc<Cell<u32>>) -> EngineRegistry {
    let registry = EngineRegistry::new();
    registry.register_factory(Rc::new(FakeFactory {
        opens: opens.clone(),
    }));
    registry.configure(config("default", "fake://a")).unwrap();
    registry.configure(config("cache", "fake://b")).unwrap();
    registry
}

#[test]
fn registry_routes_and_opens_once() {
    let opens = Rc::new(Cell::new(0));
    let registry = registry(&opens);
    registry.route("object-cache", "cache").unwrap();
    assert_eq!(registry.engine_for("object-cache"), "cache");
    assert_eq!(registry.engine_for("x"), "default");
    registry.direct("object-cache").unwrap();
    registry.direct("object-cache").unwrap();
    assert_eq!(opens.get(), 1);
    assert_eq!(
        registry.commit_order(),
        vec!["default".to_string(), "cache".to_string()]
    );
}

#[test]
fn registry_wrong_kind() {
    let opens = Rc::new(Cell::new(0));
    let registry = registry(&opens);
    registry.route("object-cache", "cache").unwrap();
    match registry.sql("object-cache") {
        Err(DbError::WrongKind { engine, expected }) => {
            assert_eq!(engine, "cache");
            assert_eq!(expected, "sql");
        }
        Err(other) => panic!("unexpected {other}"),
        Ok(_) => panic!("expected WrongKind"),
    }
}

#[test]
fn registry_rejects_unknown_scheme_and_route() {
    let opens = Rc::new(Cell::new(0));
    let registry = registry(&opens);
    assert!(matches!(
        registry.configure(config("x", "nope://a")),
        Err(DbError::Config(_))
    ));
    assert!(matches!(
        registry.configure(config("cache", "fake://c")),
        Err(DbError::Config(_))
    ));
    assert!(matches!(
        registry.route("y", "missing"),
        Err(DbError::NoEngine(_))
    ));
    assert!(registry.reset_all().is_ok());
}
