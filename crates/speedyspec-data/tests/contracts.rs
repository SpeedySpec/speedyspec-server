use std::rc::Rc;

use speedyspec_data::*;

#[allow(clippy::too_many_arguments, dead_code)]
fn takes_every_contract(
    _: &dyn SqlStore,
    _: &dyn NoSqlStore,
    _: &dyn DirectStore,
    _: &dyn Dialect,
    _: &dyn Introspector,
    _: &dyn Engine,
    _: &dyn EngineFactory,
    _: &dyn EngineLookup,
    _: &dyn UnitOfWork,
    _: &dyn PendingChange,
) {
}

#[test]
fn contracts_are_object_safe() {
    let _f: fn(
        &dyn SqlStore,
        &dyn NoSqlStore,
        &dyn DirectStore,
        &dyn Dialect,
        &dyn Introspector,
        &dyn Engine,
        &dyn EngineFactory,
        &dyn EngineLookup,
        &dyn UnitOfWork,
        &dyn PendingChange,
    ) = takes_every_contract;
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

struct Trivial;
impl SqlStore for Trivial {
    fn engine(&self) -> &str {
        "trivial"
    }
    fn dialect(&self) -> &dyn Dialect {
        &NoDialect
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            transactions: false,
            savepoints: false,
            ordered_ranges: false,
            expiry: false,
            atomic_increment: false,
            returning: false,
            upsert: UpsertForm::None,
        }
    }
    fn execute(&self, _: &Statement) -> Result<ExecResult, DbError> {
        Ok(ExecResult::default())
    }
    fn query(&self, _: &Statement) -> Result<Rows, DbError> {
        Ok(Rows::default())
    }
    fn begin(&self) -> Result<(), DbError> {
        Ok(())
    }
    fn commit(&self) -> Result<(), DbError> {
        Ok(())
    }
    fn rollback(&self) -> Result<(), DbError> {
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
        false
    }
    fn introspect(&self) -> &dyn Introspector {
        &NoIntrospector
    }
    fn reset(&self) -> Result<(), DbError> {
        Ok(())
    }
}

#[test]
fn engine_handle_accessors() {
    let handle = EngineHandle::Sql(Rc::new(Trivial));
    assert!(handle.as_sql().is_some());
    assert!(handle.as_nosql().is_none());
    assert!(handle.as_direct().is_none());
}
