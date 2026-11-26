//! A scripted fake SQL store for the `wpdb` port's tests.
#![allow(dead_code)]

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use speedyspec_data::*;
use speedyspec_data_query::mysql_text;
use speedyspec_wp_db::{MemoryWpdbHost, MemoryWpdbState, NativeWpdb, WpDbConfig, WpDbParts};
use speedyspec_wp_db_contracts::WpValue;
use speedyspec_wp_hooks::{IgnoreDeprecations, WpHookApi};
use speedyspec_wp_hooks_contracts::{Callback, HookError, HookName};
use speedyspec_wp_hooks_memory::MemoryHookStore;

pub enum Answer {
    Rows(Rows),
    Exec(ExecResult),
    Err(DbError),
}

pub struct NoDialect;
impl Dialect for NoDialect {
    fn kind(&self) -> DialectKind {
        DialectKind::MySql
    }
    fn prepare(&self, stmt: &Statement) -> Result<Prepared, DbError> {
        Ok(Prepared::Sql(vec![EmittedSql {
            sql: mysql_text(stmt.tree()),
            params: stmt.params().to_vec(),
        }]))
    }
}

pub struct NoIntrospector;
impl Introspector for NoIntrospector {
    fn server_version(&self) -> Result<String, DbError> {
        Ok("8.4.3".into())
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

/// Records each statement (its MySQL text, kind and params) and answers from a queue.
#[derive(Default)]
pub struct FakeStore {
    pub seen: RefCell<Vec<(String, StatementKind, Vec<DbValue>)>>,
    pub answers: RefCell<VecDeque<Answer>>,
    /// Answers chosen by the statement's text, checked before the queue.
    pub by_text: RefCell<Vec<(String, Rows)>>,
    pub resets: Cell<u32>,
}

impl FakeStore {
    pub fn new() -> Rc<FakeStore> {
        Rc::new(FakeStore::default())
    }
    pub fn answer(&self, answer: Answer) {
        self.answers.borrow_mut().push_back(answer);
    }
    pub fn texts(&self) -> Vec<String> {
        self.seen
            .borrow()
            .iter()
            .map(|(t, _, _)| t.clone())
            .collect()
    }
    fn next(&self, stmt: &Statement) -> Answer {
        let text = mysql_text(stmt.tree());
        self.seen
            .borrow_mut()
            .push((text.clone(), stmt.kind(), stmt.params().to_vec()));
        if let Some((_, rows)) = self
            .by_text
            .borrow()
            .iter()
            .find(|(t, _)| text.starts_with(t.as_str()))
        {
            return Answer::Rows(rows.clone());
        }
        self.answers
            .borrow_mut()
            .pop_front()
            .unwrap_or(Answer::Exec(ExecResult::default()))
    }
}

impl SqlStore for FakeStore {
    fn engine(&self) -> &str {
        "default"
    }
    fn dialect(&self) -> &dyn Dialect {
        &NoDialect
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            transactions: true,
            savepoints: true,
            ordered_ranges: true,
            expiry: false,
            atomic_increment: true,
            returning: false,
            upsert: UpsertForm::OnDuplicateKey,
        }
    }
    fn execute(&self, stmt: &Statement) -> Result<ExecResult, DbError> {
        match self.next(stmt) {
            Answer::Exec(e) => Ok(e),
            Answer::Rows(_) => Ok(ExecResult::default()),
            Answer::Err(e) => Err(e),
        }
    }
    fn query(&self, stmt: &Statement) -> Result<Rows, DbError> {
        match self.next(stmt) {
            Answer::Rows(r) => Ok(r),
            Answer::Exec(_) => Ok(Rows::default()),
            Answer::Err(e) => Err(e),
        }
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
        self.resets.set(self.resets.get() + 1);
        Ok(())
    }
}

/// Rows with these column names and text values.
pub fn rows(columns: &[&str], values: &[&[Option<&str>]]) -> Rows {
    let names: Rc<[String]> = columns.iter().map(|c| c.to_string()).collect();
    Rows {
        columns: columns
            .iter()
            .map(|c| ColumnInfo {
                name: c.to_string(),
                ..ColumnInfo::default()
            })
            .collect(),
        rows: values
            .iter()
            .map(|r| {
                Row::new(
                    names.clone(),
                    r.iter()
                        .map(|v| v.map_or(DbValue::Null, |s| DbValue::Text(s.into())))
                        .collect(),
                )
            })
            .collect(),
    }
}

pub struct Rig {
    pub db: NativeWpdb,
    pub store: Rc<FakeStore>,
    pub hooks: Rc<WpHookApi>,
    pub host: Rc<MemoryWpdbHost>,
    pub state: Rc<MemoryWpdbState>,
}

pub fn config() -> WpDbConfig {
    WpDbConfig {
        user: "root".into(),
        password: "password".into(),
        name: "wp".into(),
        host: "localhost".into(),
        charset: Some("utf8mb4".into()),
        collate: None,
        table_prefix: "wp_".into(),
    }
}

pub fn rig() -> Rig {
    rig_with(MemoryWpdbHost::new())
}

pub fn rig_with(host: MemoryWpdbHost) -> Rig {
    let store = FakeStore::new();
    let hooks = Rc::new(WpHookApi::new(
        Rc::new(MemoryHookStore::new()),
        Rc::new(IgnoreDeprecations),
    ));
    let host = Rc::new(host);
    let state = Rc::new(MemoryWpdbState::new());
    let db = NativeWpdb::new(WpDbParts {
        store: store.clone(),
        hooks: hooks.clone(),
        state: state.clone(),
        host: host.clone(),
        config: config(),
    });
    Rig {
        db,
        store,
        hooks,
        host,
        state,
    }
}

/// Adds a Rust filter callback.
pub fn filter(
    hooks: &WpHookApi,
    hook: &str,
    id: &str,
    f: impl Fn(&[WpValue]) -> Result<WpValue, HookError> + 'static,
) {
    hooks
        .add_filter(
            &HookName::from(hook),
            Callback::rust(id, f),
            &WpValue::Int(10),
            99,
        )
        .unwrap();
}
