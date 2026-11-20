use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use speedyspec_data::*;
use speedyspec_data_uow::DefaultUnitOfWork;

type Log = Rc<RefCell<Vec<String>>>;

struct NoIntrospector;
impl Introspector for NoIntrospector {
    fn server_version(&self) -> Result<String, DbError> {
        Ok(String::new())
    }
    fn tables(&self, _: Option<&str>) -> Result<Vec<String>, DbError> {
        Ok(vec![])
    }
    fn columns(&self, _: &str) -> Result<Vec<ColumnDef>, DbError> {
        Ok(vec![])
    }
    fn indexes(&self, _: &str) -> Result<Vec<IndexDef>, DbError> {
        Ok(vec![])
    }
    fn table_collation(&self, _: &str) -> Result<Option<String>, DbError> {
        Ok(None)
    }
}

struct NoDialect;
impl Dialect for NoDialect {
    fn kind(&self) -> DialectKind {
        DialectKind::MySql
    }
    fn prepare(&self, _: &Statement) -> Result<Prepared, DbError> {
        Ok(Prepared::Emulated(EmulatedOp::Noop))
    }
}

struct FakeStore {
    name: String,
    log: Log,
    open: Cell<bool>,
    fail_commit: bool,
}

impl SqlStore for FakeStore {
    fn engine(&self) -> &str {
        &self.name
    }
    fn dialect(&self) -> &dyn Dialect {
        &NoDialect
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            transactions: true,
            savepoints: true,
            ordered_ranges: false,
            expiry: false,
            atomic_increment: false,
            returning: false,
            upsert: UpsertForm::OnDuplicateKey,
        }
    }
    fn execute(&self, _: &Statement) -> Result<ExecResult, DbError> {
        Ok(ExecResult::default())
    }
    fn query(&self, _: &Statement) -> Result<Rows, DbError> {
        Ok(Rows::default())
    }
    fn begin(&self) -> Result<(), DbError> {
        self.log.borrow_mut().push(format!("begin {}", self.name));
        self.open.set(true);
        Ok(())
    }
    fn commit(&self) -> Result<(), DbError> {
        if self.fail_commit {
            return Err(DbError::Engine {
                engine: self.name.clone(),
                code: None,
                message: "boom".into(),
            });
        }
        self.log.borrow_mut().push(format!("commit {}", self.name));
        self.open.set(false);
        Ok(())
    }
    fn rollback(&self) -> Result<(), DbError> {
        self.log
            .borrow_mut()
            .push(format!("rollback {}", self.name));
        self.open.set(false);
        Ok(())
    }
    fn savepoint(&self, _: &str) -> Result<(), DbError> {
        Ok(())
    }
    fn release_savepoint(&self, _: &str) -> Result<(), DbError> {
        Ok(())
    }
    fn rollback_to_savepoint(&self, _: &str) -> Result<(), DbError> {
        Ok(())
    }
    fn in_transaction(&self) -> bool {
        self.open.get()
    }
    fn introspect(&self) -> &dyn Introspector {
        &NoIntrospector
    }
    fn reset(&self) -> Result<(), DbError> {
        Ok(())
    }
}

struct Lookup {
    stores: HashMap<String, Rc<FakeStore>>,
    order: Vec<String>,
}

impl EngineLookup for Lookup {
    fn handle(&self, engine: &str) -> Result<EngineHandle, DbError> {
        self.stores
            .get(engine)
            .map(|s| EngineHandle::Sql(s.clone()))
            .ok_or_else(|| DbError::NoEngine(engine.into()))
    }
    fn commit_order(&self) -> Vec<String> {
        self.order.clone()
    }
}

struct Change {
    engine: &'static str,
    kind: ChangeKind,
    label: &'static str,
    log: Log,
    fail: bool,
}

impl PendingChange for Change {
    fn engine(&self) -> &str {
        self.engine
    }
    fn kind(&self) -> ChangeKind {
        self.kind
    }
    fn apply(&self, _: &EngineHandle) -> Result<(), DbError> {
        if self.fail {
            return Err(DbError::Constraint {
                engine: self.engine.into(),
                message: "dup".into(),
            });
        }
        self.log.borrow_mut().push(format!("apply {}", self.label));
        Ok(())
    }
}

fn setup(engines: &[(&str, bool)]) -> (DefaultUnitOfWork, Log, HashMap<String, Rc<FakeStore>>) {
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    let stores: HashMap<String, Rc<FakeStore>> = engines
        .iter()
        .map(|(name, fail_commit)| {
            (
                name.to_string(),
                Rc::new(FakeStore {
                    name: name.to_string(),
                    log: log.clone(),
                    open: Cell::new(false),
                    fail_commit: *fail_commit,
                }),
            )
        })
        .collect();
    let lookup = Lookup {
        stores: stores.clone(),
        order: engines.iter().map(|(n, _)| n.to_string()).collect(),
    };
    (DefaultUnitOfWork::new(Rc::new(lookup)), log, stores)
}

fn change(engine: &'static str, kind: ChangeKind, label: &'static str, log: &Log) -> Box<Change> {
    Box::new(Change {
        engine,
        kind,
        label,
        log: log.clone(),
        fail: false,
    })
}

#[test]
fn applies_new_dirty_removed_in_order() {
    let (uow, log, _) = setup(&[("a", false)]);
    uow.register(change("a", ChangeKind::Removed, "r1", &log));
    uow.register(change("a", ChangeKind::Dirty, "d1", &log));
    uow.register(change("a", ChangeKind::New, "n1", &log));
    uow.register(change("a", ChangeKind::New, "n2", &log));
    let report = uow.commit().unwrap();
    assert_eq!(report.committed, vec!["a"]);
    assert_eq!(
        *log.borrow(),
        vec![
            "begin a", "apply n1", "apply n2", "apply d1", "apply r1", "commit a"
        ]
    );
    assert!(uow.is_empty());
}

#[test]
fn one_transaction_per_engine_in_commit_order() {
    let (uow, log, _) = setup(&[("a", false), ("b", false)]);
    uow.register(change("b", ChangeKind::New, "b1", &log));
    uow.register(change("a", ChangeKind::New, "a1", &log));
    uow.commit().unwrap();
    assert_eq!(
        *log.borrow(),
        vec![
            "begin a", "apply a1", "commit a", "begin b", "apply b1", "commit b"
        ]
    );
}

#[test]
fn failure_on_first_engine_is_failed_and_rolls_back() {
    let (uow, log, _) = setup(&[("a", false)]);
    uow.register(Box::new(Change {
        engine: "a",
        kind: ChangeKind::New,
        label: "x",
        log: log.clone(),
        fail: true,
    }));
    match uow.commit() {
        Err(CommitError::Failed {
            engine,
            error: DbError::Constraint { .. },
        }) => assert_eq!(engine, "a"),
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(*log.borrow(), vec!["begin a", "rollback a"]);
    assert!(uow.is_empty());
}

#[test]
fn failure_on_second_engine_is_partial() {
    let (uow, log, _) = setup(&[("a", false), ("b", true)]);
    let ran = Rc::new(Cell::new(false));
    let flag = ran.clone();
    uow.after_commit(Box::new(move || flag.set(true)));
    uow.register(change("a", ChangeKind::New, "a1", &log));
    uow.register(change("b", ChangeKind::New, "b1", &log));
    match uow.commit() {
        Err(CommitError::Partial {
            committed, failed, ..
        }) => {
            assert_eq!(committed, vec!["a"]);
            assert_eq!(failed, "b");
        }
        other => panic!("unexpected {other:?}"),
    }
    assert!(!ran.get());
    assert_eq!(log.borrow().last().unwrap(), "rollback b");
}

#[test]
fn refuses_open_outside_transaction() {
    let (uow, log, stores) = setup(&[("a", false)]);
    stores["a"].open.set(true);
    uow.register(change("a", ChangeKind::New, "a1", &log));
    match uow.commit() {
        Err(CommitError::Failed {
            engine,
            error: DbError::Transaction(message),
        }) => {
            assert_eq!(engine, "a");
            assert!(message.contains("outside the unit of work"), "{message}");
        }
        other => panic!("unexpected {other:?}"),
    }
    assert!(
        log.borrow().is_empty(),
        "nothing touched: {:?}",
        log.borrow()
    );
}

#[test]
fn callbacks_after_success_only() {
    let (uow, log, _) = setup(&[("a", false)]);
    let order: Log = Rc::new(RefCell::new(Vec::new()));
    for n in ["one", "two"] {
        let order = order.clone();
        uow.after_commit(Box::new(move || order.borrow_mut().push(n.into())));
    }
    uow.register(change("a", ChangeKind::New, "a1", &log));
    uow.commit().unwrap();
    assert_eq!(*order.borrow(), vec!["one", "two"]);

    // Discarded work runs no callbacks.
    let flag = Rc::new(Cell::new(false));
    let f = flag.clone();
    uow.after_commit(Box::new(move || f.set(true)));
    uow.register(change("a", ChangeKind::New, "a2", &log));
    uow.discard();
    assert!(uow.is_empty());
    uow.commit().unwrap();
    assert!(!flag.get());
}

#[derive(Debug, PartialEq)]
struct Opt {
    name: String,
    value: String,
}
impl Identified for Opt {
    type Id = String;
    fn identity(&self) -> String {
        self.name.clone()
    }
}

#[test]
fn identity_map_returns_same_entity() {
    let (uow, _, _) = setup(&[("a", false)]);
    let map = uow.identity_map();
    let first = map.insert(Opt {
        name: "siteurl".into(),
        value: "a".into(),
    });
    let second = map.insert(Opt {
        name: "siteurl".into(),
        value: "b".into(),
    });
    assert!(Rc::ptr_eq(&first, &second));
    assert_eq!(second.borrow().value, "a");
    assert!(Rc::ptr_eq(
        &map.get::<Opt>(&"siteurl".to_string()).unwrap(),
        &first
    ));
    map.remove::<Opt>(&"siteurl".to_string());
    assert!(map.get::<Opt>(&"siteurl".to_string()).is_none());
}
