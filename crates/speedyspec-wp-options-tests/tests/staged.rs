mod support;

use std::any::Any;
use std::rc::Rc;

use speedyspec_data::{
    ChangeKind, DbError, DirectStore, EngineHandle, EngineLookup, Health, PendingChange, UnitOfWork,
};
use speedyspec_data_uow::DefaultUnitOfWork;
use speedyspec_wp_hooks_contracts::HookValue;
use speedyspec_wp_options::{Autoload, Staged};
use support::*;

struct Dummy;

impl DirectStore for Dummy {
    fn engine(&self) -> &str {
        "memory"
    }
    fn kind(&self) -> &'static str {
        "memory"
    }
    fn connect(&self) -> Result<(), DbError> {
        Ok(())
    }
    fn health(&self) -> Result<Health, DbError> {
        Err(DbError::NoEngine("dummy".into()))
    }
    fn close(&self) -> Result<(), DbError> {
        Ok(())
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

struct Lookup;

impl EngineLookup for Lookup {
    fn handle(&self, engine: &str) -> Result<EngineHandle, DbError> {
        match engine {
            "memory" => Ok(EngineHandle::Direct(Rc::new(Dummy))),
            other => Err(DbError::NoEngine(other.into())),
        }
    }
    fn commit_order(&self) -> Vec<String> {
        vec!["memory".into()]
    }
}

struct Failing;

impl PendingChange for Failing {
    fn engine(&self) -> &str {
        "memory"
    }
    fn kind(&self) -> ChangeKind {
        ChangeKind::Removed
    }
    fn apply(&self, _: &EngineHandle) -> Result<(), DbError> {
        Err(DbError::NoEngine("forced failure".into()))
    }
}

fn uow() -> DefaultUnitOfWork {
    DefaultUnitOfWork::new(Rc::new(Lookup))
}

const ACTIONS: &[&str] = &["update_option", "update_option_x", "updated_option"];

#[test]
fn filters_at_registration_actions_after_commit() {
    let f = fixture();
    seed(&f, "x", "old", Autoload::On);
    record(&f, "pre_update_option_x");
    for h in ACTIONS {
        record(&f, h);
    }
    let u = uow();
    assert_eq!(
        f.api
            .stage_update(&u, "x", s("new"), HookValue::Null)
            .unwrap(),
        Staged::Registered
    );
    assert_eq!(hooks_run(&f), ["pre_update_option_x"]);
    assert_eq!(stored(&f, "x"), Some(b"old".to_vec()));
    u.commit().unwrap();
    assert_eq!(
        hooks_run(&f),
        [
            "pre_update_option_x",
            "update_option",
            "update_option_x",
            "updated_option"
        ]
    );
    assert_eq!(stored(&f, "x"), Some(b"new".to_vec()));
    assert_eq!(f.api.get_option("x", None).unwrap(), s("new"));
}

#[test]
fn failed_commit_fires_nothing() {
    let f = fixture();
    seed(&f, "x", "old", Autoload::On);
    f.api.get_option("x", None).unwrap();
    let before = cached(&f, "alloptions");
    for h in ["update_option_x", "updated_option"] {
        record(&f, h);
    }
    let u = uow();
    f.api
        .stage_update(&u, "x", s("new"), HookValue::Null)
        .unwrap();
    u.register(Box::new(Failing));
    assert!(u.commit().is_err());
    assert!(hooks_run(&f).is_empty());
    assert_eq!(cached(&f, "alloptions"), before);
    assert_eq!(cached(&f, "x"), None);
    assert_eq!(f.api.get_option("x", None).unwrap(), s("old"));
}

#[test]
fn registration_order_across_options() {
    let f = fixture();
    seed(&f, "a", "1", Autoload::On);
    seed(&f, "b", "1", Autoload::On);
    record(&f, "updated_option");
    let u = uow();
    f.api
        .stage_update(&u, "a", s("2"), HookValue::Null)
        .unwrap();
    f.api
        .stage_update(&u, "b", s("2"), HookValue::Null)
        .unwrap();
    u.commit().unwrap();
    let log = f.log.borrow();
    let names: Vec<HookValue> = log.iter().map(|(_, args)| args[0].clone()).collect();
    assert_eq!(names, vec![s("a"), s("b")]);
}

#[test]
fn second_stage_sees_first() {
    let f = fixture();
    seed(&f, "x", "0", Autoload::On);
    record(&f, "pre_update_option_x");
    let u = uow();
    f.api
        .stage_update(&u, "x", s("1"), HookValue::Null)
        .unwrap();
    f.api
        .stage_update(&u, "x", s("2"), HookValue::Null)
        .unwrap();
    {
        let log = f.log.borrow();
        assert_eq!(
            log[1].1[1],
            s("1"),
            "the second stage's old value is the first's"
        );
    }
    let before = f.repo.statements();
    u.commit().unwrap();
    // One repository write, of the second value.
    assert_eq!(f.repo.statements() - before, 1);
    assert_eq!(stored(&f, "x"), Some(b"2".to_vec()));
    assert_eq!(f.api.get_option("x", None).unwrap(), s("2"));
}

#[test]
fn unchanged_is_reported() {
    let f = fixture();
    seed(&f, "x", "v", Autoload::On);
    let u = uow();
    assert_eq!(
        f.api
            .stage_update(&u, "x", s("v"), HookValue::Null)
            .unwrap(),
        Staged::Unchanged
    );
    assert!(u.is_empty());
    assert_eq!(
        f.api
            .stage_update(&u, " ", s("v"), HookValue::Null)
            .unwrap(),
        Staged::Rejected
    );
    assert_eq!(
        f.api.stage_add(&u, "x", s("w"), HookValue::Null).unwrap(),
        Staged::Exists
    );
    assert_eq!(f.api.stage_delete(&u, "nope").unwrap(), Staged::Missing);
    assert!(u.is_empty());
}

#[test]
fn staged_add_and_delete_commit() {
    let f = fixture();
    seed(&f, "gone", "v", Autoload::On);
    for h in ["added_option", "deleted_option"] {
        record(&f, h);
    }
    let u = uow();
    assert_eq!(
        f.api.stage_add(&u, "new", s("n"), HookValue::Null).unwrap(),
        Staged::Registered
    );
    assert_eq!(f.api.stage_delete(&u, "gone").unwrap(), Staged::Registered);
    assert!(hooks_run(&f).is_empty());
    u.commit().unwrap();
    assert_eq!(hooks_run(&f), ["added_option", "deleted_option"]);
    assert_eq!(stored(&f, "new"), Some(b"n".to_vec()));
    assert_eq!(stored(&f, "gone"), None);
}

fn autoload_of(f: &Fixture, n: &str) -> Option<Autoload> {
    use speedyspec_wp_options::{OptionName, OptionRepository};
    f.repo.autoload_of(&OptionName::new(n).unwrap()).unwrap()
}

#[test]
fn delete_then_update_writes_the_update() {
    let f = fixture();
    seed(&f, "x", "a", Autoload::On);
    let u = uow();
    assert_eq!(f.api.stage_delete(&u, "x").unwrap(), Staged::Registered);
    assert_eq!(
        f.api
            .stage_update(&u, "x", s("b"), HookValue::Null)
            .unwrap(),
        Staged::Registered
    );
    u.commit().unwrap();
    assert_eq!(stored(&f, "x"), Some(b"b".to_vec()));
    assert_eq!(f.api.get_option("x", None).unwrap(), s("b"));
}

#[test]
fn delete_then_add_writes_the_add() {
    let f = fixture();
    seed(&f, "x", "a", Autoload::On);
    let u = uow();
    f.api.stage_delete(&u, "x").unwrap();
    assert_eq!(
        f.api.stage_add(&u, "x", s("a"), HookValue::Null).unwrap(),
        Staged::Registered
    );
    u.commit().unwrap();
    assert_eq!(stored(&f, "x"), Some(b"a".to_vec()));
    assert_eq!(f.api.get_option("x", None).unwrap(), s("a"));
}

#[test]
fn add_then_delete_cancels() {
    let f = fixture();
    let u = uow();
    assert_eq!(
        f.api.stage_add(&u, "y", s("v"), HookValue::Null).unwrap(),
        Staged::Registered
    );
    assert_eq!(f.api.stage_delete(&u, "y").unwrap(), Staged::Registered);
    assert_eq!(
        f.api.stage_add(&u, "z", s("v"), HookValue::Null).unwrap(),
        Staged::Registered
    );
    assert_eq!(
        f.api.stage_add(&u, "z", s("w"), HookValue::Null).unwrap(),
        Staged::Exists
    );
    u.commit().unwrap();
    assert_eq!(stored(&f, "y"), None);
    assert_eq!(f.api.get_option("y", None).unwrap(), HookValue::Bool(false));
}

#[test]
fn second_update_keeps_the_first_autoload() {
    let f = fixture();
    // Another autoloaded row: with none left, wp_load_alloptions() falls back to every row, as WordPress's does.
    seed(&f, "keep", "k", Autoload::On);
    seed(&f, "z", "a", Autoload::On);
    let u = uow();
    f.api.stage_update(&u, "z", s("b"), s("off")).unwrap();
    f.api
        .stage_update(&u, "z", s("c"), HookValue::Null)
        .unwrap();
    u.commit().unwrap();
    assert_eq!(stored(&f, "z"), Some(b"c".to_vec()));
    assert_eq!(autoload_of(&f, "z"), Some(Autoload::Off));
    assert_eq!(in_alloptions(&f, "z"), None);
    assert_eq!(cached(&f, "z"), Some(s("c")));
}
