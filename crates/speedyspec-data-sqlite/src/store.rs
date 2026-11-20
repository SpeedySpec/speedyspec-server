//! `SqliteStore`: MySQL statements run on SQLite, answered with MySQL's counts, ids and results.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rusqlite::Connection;
use rusqlite::types::Value as SqlValue;
use speedyspec_data::{
    Capabilities, ColumnDef, DbError, DbValue, Dialect, EngineConfig, ExecResult, IndexDef,
    Introspector, Rows, SqlStore, Statement, StatementKind, TxControl, UpsertForm,
};
use speedyspec_data_translate::{
    CatalogueTable, SessionVars, answer_emulated, catalogue_ddl, catalogue_tables,
    is_catalogue_table, mysql_like, object_name, read_catalogue,
};
use sqlparser::ast::{Set, Statement as Ast, TableObject};

use crate::connection::{SqliteTarget, open_connection};
use crate::errors::map_error;
use crate::plan::{CountRule, InsertIdRule, SqlitePlan, SqliteStep};
use crate::results::{Origin, catalogue_column, describe, rows_of, value};
use crate::{SqliteDialect, info_schema, introspect};

/// MySQL's version, as SQLite-backed SpeedySpec reports it.
const MYSQL_VERSION: &str = "8.4.0";

pub struct SqliteStore {
    name: String,
    conn: Connection,
    dialect: SqliteDialect,
    session: Arc<Mutex<SessionVars>>,
    busy_timeout: Duration,
    version: String,
    found_rows: Cell<u64>,
    /// `SET autocommit = 0`: a transaction is always open, a new one after each commit or rollback.
    autocommit_off: Cell<bool>,
    /// Running the actions of one multi-action ALTER TABLE, which commits once, at its end.
    altering: Cell<bool>,
    /// This connection's temporary tables, as MySQL would describe them; never in the shared catalogue.
    temporary: RefCell<HashMap<String, CatalogueTable>>,
    catalogue: RefCell<HashMap<String, Option<CatalogueTable>>>,
}

/// What one step did.
struct Outcome {
    rows: Option<Rows>,
    changes: u64,
    moved: bool,
    rowid: i64,
    first_returned: Option<i64>,
}

fn target(url: &str) -> Result<SqliteTarget, DbError> {
    match url.strip_prefix("sqlite://") {
        Some(":memory:") => Ok(SqliteTarget::Memory),
        Some(path) if path.starts_with('/') => Ok(SqliteTarget::File(PathBuf::from(path))),
        _ => Err(DbError::Config(format!(
            "a SQLite URL is sqlite://:memory: or sqlite:///<absolute path>, not `{url}`"
        ))),
    }
}

fn to_sql(v: &DbValue) -> SqlValue {
    match v {
        DbValue::Null => SqlValue::Null,
        DbValue::Int(i) => SqlValue::Integer(*i),
        DbValue::UInt(u) => {
            i64::try_from(*u).map_or_else(|_| SqlValue::Text(u.to_string()), SqlValue::Integer)
        }
        DbValue::Float(f) => SqlValue::Real(*f),
        DbValue::Text(s) => SqlValue::Text(s.clone()),
        DbValue::Bytes(b) => SqlValue::Blob(b.clone()),
    }
}

/// `SET autocommit = 0|1`, which SQLite has no variable for.
fn autocommit(stmt: &Statement) -> Option<bool> {
    let Ast::Set(Set::SingleAssignment {
        variable, values, ..
    }) = stmt.tree()
    else {
        return None;
    };
    let name = object_name(variable).to_ascii_lowercase();
    let name = name
        .trim_start_matches("@@")
        .trim_start_matches("session.")
        .trim_start_matches("local.");
    if name != "autocommit" {
        return None;
    }
    match values
        .first()?
        .to_string()
        .to_ascii_lowercase()
        .trim_matches('\'')
    {
        "0" | "off" | "false" => Some(false),
        "1" | "on" | "true" => Some(true),
        _ => None,
    }
}

/// Temporary tables are the DDL MySQL does not commit for.
fn commits_implicitly(stmt: &Statement) -> bool {
    stmt.kind() == StatementKind::Ddl
        && !matches!(stmt.tree(), Ast::CreateTable(c) if c.temporary)
        && !matches!(
            stmt.tree(),
            Ast::Drop {
                temporary: true,
                ..
            }
        )
}

fn insert_table(stmt: &Statement) -> Option<String> {
    match stmt.tree() {
        Ast::Insert(insert) => match &insert.table {
            TableObject::TableName(name) => Some(object_name(name)),
            _ => None,
        },
        _ => None,
    }
}

impl SqliteStore {
    pub fn open(config: &EngineConfig) -> Result<SqliteStore, DbError> {
        let target = target(&config.url)?;
        let busy_timeout = Duration::from_millis(
            config
                .options
                .get("busy_timeout_ms")
                .map(|v| {
                    v.parse().map_err(|_| {
                        DbError::Config(format!("busy_timeout_ms `{v}` is not a number"))
                    })
                })
                .transpose()?
                .unwrap_or(5000),
        );
        let database = config
            .options
            .get("database")
            .cloned()
            .unwrap_or_else(|| match &target {
                SqliteTarget::Memory => "main".to_string(),
                SqliteTarget::File(path) => path
                    .file_stem()
                    .map_or("main".into(), |s| s.to_string_lossy().into_owned()),
            });
        let probe = Connection::open_in_memory().map_err(|e| map_error(&config.name, e))?;
        let version = format!(
            "{MYSQL_VERSION}-speedyspec-sqlite-{}",
            introspect::sqlite_version(&probe)?
        );
        let session = Arc::new(Mutex::new(SessionVars::mysql_defaults(&database, &version)));
        let conn =
            open_connection(&target, busy_timeout, session.clone()).map_err(|e| match e {
                DbError::Engine {
                    code: Some(c),
                    message,
                    ..
                } if c == "SQLITE_CANTOPEN" => DbError::Connection {
                    engine: config.name.clone(),
                    message,
                },
                other => other,
            })?;
        conn.execute_batch(info_schema::CREATE)
            .map_err(|e| map_error(&config.name, e))?;
        let store = SqliteStore {
            name: config.name.clone(),
            conn,
            dialect: SqliteDialect::new(),
            session,
            busy_timeout,
            version,
            found_rows: Cell::new(0),
            autocommit_off: Cell::new(false),
            altering: Cell::new(false),
            temporary: RefCell::new(HashMap::new()),
            catalogue: RefCell::new(HashMap::new()),
        };
        for stmt in catalogue_ddl() {
            store.run(&stmt)?;
        }
        // TRUNCATE resets counters in sqlite_sequence, which SQLite creates with the first AUTOINCREMENT table.
        store
            .conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS _speedyspec_sequence (id INTEGER PRIMARY KEY AUTOINCREMENT); DROP TABLE _speedyspec_sequence;",
            )
            .map_err(|e| store.error(e))?;
        Ok(store)
    }

    pub fn plan(&self, stmt: &Statement) -> Result<SqlitePlan, DbError> {
        self.dialect.plan(stmt, Some(self))
    }

    pub fn busy_timeout(&self) -> Duration {
        self.busy_timeout
    }

    fn error(&self, e: rusqlite::Error) -> DbError {
        match map_error(&self.name, e) {
            // MySQL's wording, which WordPress and plugins match on.
            DbError::Engine {
                engine, message, ..
            } if message.starts_with("no such table: ") => {
                let table = message.trim_start_matches("no such table: ");
                let table = table.rsplit('.').next().unwrap_or(table);
                let database = self.session.lock().expect("session").database().to_string();
                DbError::Engine {
                    engine,
                    code: Some("1146".into()),
                    message: format!("Table '{database}.{table}' doesn't exist"),
                }
            }
            other => other,
        }
    }

    fn control(&self, sql: &str) -> Result<(), DbError> {
        self.conn.execute_batch(sql).map_err(|e| self.error(e))
    }

    /// The catalogue's record of `table`, cached until the next DDL statement.
    fn catalogue_of(&self, table: &str) -> Result<Option<CatalogueTable>, DbError> {
        if let Some(temp) = self.temporary.borrow().get(table) {
            // A temporary table hides the permanent one of its name, as in MySQL.
            return Ok(Some(temp.clone()));
        }
        let key = table.to_string();
        if let Some(hit) = self.catalogue.borrow().get(&key) {
            return Ok(hit.clone());
        }
        let found = read_catalogue(&|s| self.internal(s), table)?;
        self.catalogue.borrow_mut().insert(key, found.clone());
        Ok(found)
    }

    /// Runs a statement of the catalogue's own, planned without the schema (which reading it would need).
    fn internal(&self, stmt: &Statement) -> Result<Rows, DbError> {
        match self.dialect.plan(stmt, None)? {
            SqlitePlan::Run { steps, .. } => {
                let mut rows = Rows::default();
                for step in &steps {
                    let outcome = self.step(step)?;
                    if step.returns_rows {
                        rows = outcome.rows.unwrap_or_default();
                    }
                }
                Ok(rows)
            }
            SqlitePlan::Emulated(op) => Err(DbError::Unsupported {
                construct: format!("{op:?} inside the catalogue"),
                dialect: speedyspec_data::DialectKind::Sqlite,
            }),
        }
    }

    /// The origin of each result column, with its catalogue column when there is one.
    fn origins(&self, stmt: &rusqlite::Statement<'_>) -> Result<Vec<Option<Origin>>, DbError> {
        let meta: Vec<(Option<String>, Option<String>, Option<String>)> = stmt
            .columns_with_metadata()
            .map(|m| {
                (
                    m.database_name().map(str::to_string),
                    m.table_name().map(str::to_string),
                    m.origin_name().map(str::to_string),
                )
            })
            .collect();
        meta.into_iter()
            .map(
                |(database, table, column)| match (database.as_deref(), table, column) {
                    (Some("main"), Some(table), Some(column)) if !is_catalogue_table(&table) => {
                        let catalogue = self
                            .catalogue_of(&table)?
                            .and_then(|t| catalogue_column(&t, &column));
                        Ok(Some(Origin {
                            table,
                            column,
                            catalogue,
                        }))
                    }
                    _ => Ok(None),
                },
            )
            .collect()
    }

    fn step(&self, step: &SqliteStep) -> Result<Outcome, DbError> {
        let mut stmt = self
            .conn
            .prepare(&step.sql.sql)
            .map_err(|e| self.error(e))?;
        for i in 1..=stmt.parameter_count() {
            let value = step.sql.params.get(i - 1).ok_or_else(|| DbError::Engine {
                engine: self.name.clone(),
                code: None,
                message: format!("no value for parameter {i}"),
            })?;
            stmt.raw_bind_parameter(i, to_sql(value))
                .map_err(|e| self.error(e))?;
        }
        let before = self.conn.last_insert_rowid();
        let mut outcome = Outcome {
            rows: None,
            changes: 0,
            moved: false,
            rowid: 0,
            first_returned: None,
        };
        if stmt.column_count() > 0 {
            let origins = self.origins(&stmt)?;
            let names: Vec<String> = stmt.column_names().map(str::to_string).collect();
            let mut values = Vec::new();
            let mut rows = stmt.raw_query();
            while let Some(row) = rows.next().map_err(|e| self.error(e))? {
                let mut out = Vec::with_capacity(names.len());
                for (i, origin) in origins.iter().enumerate() {
                    let raw = row.get_ref(i).map_err(|e| self.error(e))?;
                    out.push(value(
                        raw,
                        origin
                            .as_ref()
                            .and_then(|o| o.catalogue.as_ref())
                            .map(|(c, _)| c),
                    ));
                }
                values.push(out);
            }
            drop(rows);
            outcome.first_returned = values.first().and_then(|r| match r.first() {
                Some(DbValue::Int(i)) => Some(*i),
                _ => None,
            });
            let rows = rows_of(&names, values);
            outcome.rows = Some(Rows {
                columns: describe(&names, &origins, &rows),
                rows,
            });
        } else {
            stmt.raw_execute().map_err(|e| self.error(e))?;
        }
        outcome.changes = self.conn.changes();
        outcome.rowid = self.conn.last_insert_rowid();
        outcome.moved = outcome.rowid != before;
        Ok(outcome)
    }

    /// Runs every step as one statement: inside a transaction of its own, or a savepoint in the open one.
    fn steps(&self, steps: &[SqliteStep]) -> Result<Vec<Outcome>, DbError> {
        if steps.len() < 2 {
            return steps.iter().map(|s| self.step(s)).collect();
        }
        let own = self.conn.is_autocommit();
        self.control(if own {
            "BEGIN IMMEDIATE"
        } else {
            "SAVEPOINT speedyspec_stmt"
        })?;
        let result: Result<Vec<Outcome>, DbError> = steps.iter().map(|s| self.step(s)).collect();
        match result {
            Ok(outcomes) => {
                self.control(if own {
                    "COMMIT"
                } else {
                    "RELEASE speedyspec_stmt"
                })?;
                Ok(outcomes)
            }
            Err(e) => {
                let _ = self.control(if own {
                    "ROLLBACK"
                } else {
                    "ROLLBACK TO speedyspec_stmt; RELEASE speedyspec_stmt"
                });
                Err(e)
            }
        }
    }

    /// After a commit or rollback with autocommit off, MySQL has a new transaction open.
    fn reopen(&self) -> Result<(), DbError> {
        if self.autocommit_off.get() && self.conn.is_autocommit() {
            self.control("BEGIN IMMEDIATE")?;
        }
        Ok(())
    }

    fn set_autocommit(&self, on: bool) -> Result<(), DbError> {
        self.autocommit_off.set(!on);
        if on && !self.conn.is_autocommit() {
            self.control("COMMIT")?;
        }
        self.reopen()?;
        self.session
            .lock()
            .expect("session")
            .set("autocommit", if on { "ON" } else { "OFF" });
        Ok(())
    }

    fn auto_increment(&self, table: &str) -> Result<bool, DbError> {
        Ok(self
            .columns(table)?
            .iter()
            .any(|c| c.extra == "auto_increment"))
    }

    /// An `ALTER TABLE` of several actions, each planned against the schema the one before it left, as one
    /// statement: MySQL applies the actions in order.
    fn alter_in_order(&self, stmt: &Statement) -> Result<(Rows, ExecResult), DbError> {
        let Ast::AlterTable(alter) = stmt.tree() else {
            unreachable!("an ALTER TABLE")
        };
        if !self.conn.is_autocommit() {
            self.control("COMMIT")?;
        }
        self.control("BEGIN IMMEDIATE")?;
        for op in &alter.operations {
            let mut one = alter.clone();
            one.operations = vec![op.clone()];
            let part = Statement::new(Ast::AlterTable(one), stmt.params().to_vec());
            self.altering.set(true);
            let result = self.run(&part);
            self.altering.set(false);
            if let Err(e) = result {
                let _ = self.control("ROLLBACK");
                self.catalogue.borrow_mut().clear();
                return Err(e);
            }
        }
        self.control("COMMIT")?;
        self.reopen()?;
        Ok((Rows::default(), ExecResult::default()))
    }

    /// Records or forgets a temporary table this connection made or dropped.
    fn track_temporary(&self, stmt: &Statement) -> Result<(), DbError> {
        match stmt.tree() {
            Ast::CreateTable(create) if create.temporary => {
                let table = speedyspec_data_translate::catalogue_table(stmt.tree())?;
                self.temporary
                    .borrow_mut()
                    .insert(table.name.clone(), table);
            }
            Ast::Drop { names, .. } => {
                for name in names {
                    self.temporary.borrow_mut().remove(&object_name(name));
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn run(&self, stmt: &Statement) -> Result<(Rows, ExecResult), DbError> {
        if let Some(on) = autocommit(stmt) {
            self.set_autocommit(on)?;
            return Ok((Rows::default(), ExecResult::default()));
        }
        if let Ast::AlterTable(alter) = stmt.tree()
            && alter.operations.len() > 1
        {
            return self.alter_in_order(stmt);
        }
        let begins = stmt.kind() == StatementKind::Transaction(TxControl::Begin);
        // MySQL commits the open transaction before DDL and before a new START TRANSACTION.
        if (commits_implicitly(stmt) || begins)
            && !self.conn.is_autocommit()
            && !self.altering.get()
        {
            self.control("COMMIT")?;
        }
        let plan = self.plan(stmt);
        let plan = match plan {
            Ok(plan) => plan,
            Err(e) => {
                self.reopen()?;
                return Err(match e {
                    // A pass's engine error names no engine; this store is the one that refused.
                    DbError::Engine {
                        engine,
                        code,
                        message,
                    } if engine.is_empty() => DbError::Engine {
                        engine: self.name.clone(),
                        code,
                        message,
                    },
                    other => other,
                });
            }
        };
        let (steps, found_rows, information_schema) = match plan {
            SqlitePlan::Emulated(speedyspec_data::EmulatedOp::SetSession { name, value })
                if name == "database" =>
            {
                let database = self.session.lock().expect("session").database().to_string();
                if !value.eq_ignore_ascii_case(&database) {
                    return Err(DbError::Engine {
                        engine: self.name.clone(),
                        code: Some("1049".into()),
                        message: format!("Unknown database '{value}'"),
                    });
                }
                return Ok((Rows::default(), ExecResult::default()));
            }
            SqlitePlan::Emulated(op) => {
                let rows = {
                    let found = self.found_rows.get();
                    let mut session = self.session.lock().expect("session");
                    answer_emulated(&op, self, &mut session, found)?
                };
                return Ok((rows, ExecResult::default()));
            }
            SqlitePlan::Run {
                steps,
                found_rows,
                information_schema,
            } => (steps, found_rows, information_schema),
        };
        if information_schema {
            let database = self.session.lock().expect("session").database().to_string();
            info_schema::refresh(&self.conn, self, &database)?;
        }
        let kind = stmt.kind();
        let result = self.steps(&steps);
        if kind == StatementKind::Ddl {
            self.catalogue.borrow_mut().clear();
        }
        if matches!(
            kind,
            StatementKind::Transaction(TxControl::Commit | TxControl::Rollback)
        ) || kind == StatementKind::Ddl
        {
            self.reopen()?;
        }
        let outcomes = result?;
        self.track_temporary(stmt)?;
        let auto = match insert_table(stmt) {
            Some(table) => self.auto_increment(&table)?,
            None => false,
        };
        let mut exec = ExecResult::default();
        let mut rows = None;
        for (step, outcome) in steps.iter().zip(outcomes) {
            exec.rows_affected += match step.count {
                CountRule::Changes => outcome.changes,
                CountRule::Upsert if outcome.changes == 0 => 0,
                CountRule::Upsert if outcome.moved => 1,
                CountRule::Upsert => 2,
                CountRule::None => 0,
            };
            if auto && exec.insert_id.is_none() && outcome.changes > 0 {
                exec.insert_id = match step.insert_id {
                    InsertIdRule::LastRowid => u64::try_from(outcome.rowid).ok(),
                    InsertIdRule::Returning => {
                        outcome.first_returned.and_then(|r| u64::try_from(r).ok())
                    }
                    _ => None,
                };
            }
            if step.returns_rows {
                rows = outcome.rows;
            }
        }
        let rows = rows.unwrap_or_default();
        if !steps.iter().any(|s| s.returns_rows) {
            return Ok((rows, exec));
        }
        match found_rows {
            Some(count) => {
                let counted = self.step(&SqliteStep {
                    sql: count,
                    count: CountRule::None,
                    insert_id: InsertIdRule::None,
                    returns_rows: true,
                })?;
                let n = counted
                    .rows
                    .and_then(|r| r.rows.first().and_then(|row| row.get_index(0).cloned()))
                    .and_then(|v| match v {
                        DbValue::Int(i) => u64::try_from(i).ok(),
                        _ => None,
                    })
                    .unwrap_or(0);
                self.found_rows.set(n);
            }
            None => self.found_rows.set(rows.rows.len() as u64),
        }
        exec.rows_affected = rows.rows.len() as u64;
        Ok((rows, exec))
    }
}

impl Introspector for SqliteStore {
    fn server_version(&self) -> Result<String, DbError> {
        Ok(self.version.clone())
    }

    fn tables(&self, like: Option<&str>) -> Result<Vec<String>, DbError> {
        let mut tables = catalogue_tables(&|s| self.internal(s))?;
        for table in introspect::engine_tables(&self.conn).map_err(|e| self.error(e))? {
            if !tables.iter().any(|t| t == &table) {
                tables.push(table);
            }
        }
        tables.retain(|t| !is_catalogue_table(t) && like.is_none_or(|p| mysql_like(p, t, false)));
        tables.sort();
        Ok(tables)
    }

    fn columns(&self, table: &str) -> Result<Vec<ColumnDef>, DbError> {
        if is_catalogue_table(table) {
            return introspect::pragma_columns(&self.conn, table).map_err(|e| self.error(e));
        }
        match self.catalogue_of(table)? {
            Some(t) => Ok(t.column_defs()),
            None => introspect::pragma_columns(&self.conn, table).map_err(|e| self.error(e)),
        }
    }

    fn indexes(&self, table: &str) -> Result<Vec<IndexDef>, DbError> {
        if is_catalogue_table(table) {
            return introspect::pragma_indexes(&self.conn, table).map_err(|e| self.error(e));
        }
        match self.catalogue_of(table)? {
            Some(t) => Ok(t.index_defs()),
            None => introspect::pragma_indexes(&self.conn, table).map_err(|e| self.error(e)),
        }
    }

    fn table_collation(&self, table: &str) -> Result<Option<String>, DbError> {
        Ok(self.catalogue_of(table)?.and_then(|t| t.collation))
    }

    fn index_type(&self, table: &str, index: &str) -> Result<Option<String>, DbError> {
        Ok(self.catalogue_of(table)?.and_then(|t| t.index_type(index)))
    }
}

impl SqlStore for SqliteStore {
    fn engine(&self) -> &str {
        &self.name
    }

    fn dialect(&self) -> &dyn Dialect {
        &self.dialect
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            transactions: true,
            savepoints: true,
            ordered_ranges: false,
            expiry: false,
            atomic_increment: true,
            returning: true,
            upsert: UpsertForm::OnConflict,
        }
    }

    fn execute(&self, stmt: &Statement) -> Result<ExecResult, DbError> {
        self.run(stmt).map(|(_, exec)| exec)
    }

    fn query(&self, stmt: &Statement) -> Result<Rows, DbError> {
        self.run(stmt).map(|(rows, _)| rows)
    }

    fn begin(&self) -> Result<(), DbError> {
        if !self.conn.is_autocommit() {
            // MySQL's BEGIN commits the open transaction first.
            self.control("COMMIT")?;
        }
        self.control("BEGIN IMMEDIATE")
    }

    fn commit(&self) -> Result<(), DbError> {
        if !self.conn.is_autocommit() {
            self.control("COMMIT")?;
        }
        self.reopen()
    }

    fn rollback(&self) -> Result<(), DbError> {
        if !self.conn.is_autocommit() {
            self.control("ROLLBACK")?;
        }
        self.catalogue.borrow_mut().clear();
        self.reopen()
    }

    fn savepoint(&self, name: &str) -> Result<(), DbError> {
        self.control(&format!("SAVEPOINT \"{}\"", name.replace('"', "\"\"")))
    }

    fn release_savepoint(&self, name: &str) -> Result<(), DbError> {
        self.control(&format!("RELEASE \"{}\"", name.replace('"', "\"\"")))
    }

    fn rollback_to_savepoint(&self, name: &str) -> Result<(), DbError> {
        self.control(&format!("ROLLBACK TO \"{}\"", name.replace('"', "\"\"")))
    }

    fn in_transaction(&self) -> bool {
        !self.conn.is_autocommit()
    }

    fn introspect(&self) -> &dyn Introspector {
        self
    }

    fn reset(&self) -> Result<(), DbError> {
        self.autocommit_off.set(false);
        if !self.conn.is_autocommit() {
            self.control("ROLLBACK")?;
        }
        self.session.lock().expect("session").reset();
        self.found_rows.set(0);
        self.catalogue.borrow_mut().clear();
        // A connection reset ends its temporary tables, as MySQL's does.
        self.temporary.borrow_mut().clear();
        let scratch: Vec<String> = {
            let mut stmt = self
                .conn
                .prepare("SELECT name FROM temp.sqlite_master WHERE type = 'table'")
                .map_err(|e| self.error(e))?;
            stmt.query_map([], |r| r.get::<_, String>(0))
                .map_err(|e| self.error(e))?
                .collect::<Result<_, _>>()
                .map_err(|e| self.error(e))?
        };
        for table in scratch {
            self.control(&format!(
                "DROP TABLE temp.\"{}\"",
                table.replace('"', "\"\"")
            ))?;
        }
        Ok(())
    }
}
