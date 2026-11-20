//! `PostgresStore` (Adapter): MySQL statements run on PostgreSQL, answered with MySQL's counts, ids and results.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use speedyspec_data::{
    Capabilities, DbError, DbValue, Dialect, EmulatedOp, EngineConfig, ExecResult, Introspector,
    Rows, SqlStore, Statement, StatementKind, TxControl, UpsertForm,
};
use speedyspec_data_translate::{SessionVars, answer_emulated, catalogue_ddl, object_name};
use sqlparser::ast::{Expr, SelectItem, Set, SetExpr, Statement as Ast};

use crate::connection::Connection;
use crate::dialect::PostgresDialect;
use crate::exec;
use crate::introspect::PostgresIntrospector;
use crate::plan::{ID_ALIAS, INSERTED_ALIAS, PgEmitted, PgPlan, RowCount};

const MYSQL_VERSION: &str = "8.4.0";

pub struct PostgresStore {
    name: String,
    conn: Rc<RefCell<Connection>>,
    introspector: Rc<PostgresIntrospector>,
    dialect: PostgresDialect,
    session: RefCell<SessionVars>,
    in_tx: Cell<bool>,
    autocommit_off: Cell<bool>,
    found_rows: Cell<u64>,
    last_id: Cell<u64>,
    /// (table OID, column number) → (table, column), forgotten after DDL.
    origins: RefCell<std::collections::HashMap<(u32, i16), Option<(String, String)>>>,
}

/// `SET autocommit = 0|1`, which PostgreSQL has no variable for.
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

/// `SELECT LAST_INSERT_ID()` alone, which the store answers from its own last id.
fn is_last_insert_id(stmt: &Statement) -> bool {
    let Ast::Query(query) = stmt.tree() else {
        return false;
    };
    let SetExpr::Select(select) = query.body.as_ref() else {
        return false;
    };
    select.from.is_empty()
        && matches!(select.projection.as_slice(), [SelectItem::UnnamedExpr(Expr::Function(f)) | SelectItem::ExprWithAlias { expr: Expr::Function(f), .. }]
            if object_name(&f.name).eq_ignore_ascii_case("last_insert_id") && matches!(&f.args, sqlparser::ast::FunctionArguments::List(l) if l.args.is_empty()))
}

fn int(v: &DbValue) -> Option<i64> {
    match v {
        DbValue::Int(i) => Some(*i),
        DbValue::Text(s) => s.parse().ok(),
        _ => None,
    }
}

fn one_value(name: &str, value: DbValue) -> Rows {
    let names: Rc<[String]> = Rc::from(vec![name.to_string()]);
    Rows {
        columns: vec![speedyspec_data::ColumnInfo {
            name: name.to_string(),
            sql_type: "MYSQL_TYPE_LONGLONG".into(),
            flags: 32768,
            charset: 63,
            ..Default::default()
        }],
        rows: vec![speedyspec_data::Row::new(names, vec![value])],
    }
}

impl PostgresStore {
    pub fn open(config: &EngineConfig) -> Result<PostgresStore, DbError> {
        if let Some(mode) = config.options.get("sslmode")
            && mode != "disable"
        {
            return Err(DbError::Config(
                "TLS for postgres is not supported yet".into(),
            ));
        }
        let schema = config
            .options
            .get("schema")
            .cloned()
            .unwrap_or_else(|| "public".into());
        let mut conn = Connection::new(&config.name, &config.url, &schema)?;
        conn.set_application_name(
            config
                .options
                .get("application_name")
                .map_or("speedyspec", String::as_str),
        );
        let database = conn.database();
        let conn = Rc::new(RefCell::new(conn));
        let introspector = Rc::new(PostgresIntrospector::new(conn.clone()));
        let version = format!("{MYSQL_VERSION}-speedyspec-postgres");
        let store = PostgresStore {
            name: config.name.clone(),
            dialect: PostgresDialect::new(Some(introspector.clone() as Rc<dyn Introspector>)),
            conn,
            introspector,
            session: RefCell::new(SessionVars::mysql_defaults(&database, &version)),
            in_tx: Cell::new(false),
            autocommit_off: Cell::new(false),
            found_rows: Cell::new(0),
            last_id: Cell::new(0),
            origins: RefCell::new(std::collections::HashMap::new()),
        };
        if let Ok(v) = store.introspector.server_version() {
            store.session.borrow_mut().set("version", &v);
        }
        for stmt in catalogue_ddl() {
            store.run(&stmt)?;
        }
        store.sync_session()?;
        Ok(store)
    }

    /// The session variables, where `@@name` (`speedyspec_sysvar`) reads them.
    fn sync_session(&self) -> Result<(), DbError> {
        let values: Vec<(String, String)> = self.session.borrow().like(None);
        let sql: Vec<String> = values
            .iter()
            .map(|(k, v)| {
                format!(
                    "SELECT set_config('speedyspec.{}', '{}', false)",
                    k.replace('\'', ""),
                    v.replace('\'', "''")
                )
            })
            .collect();
        self.control(&sql.join("; "))
    }

    /// The table and column a result column comes from.
    fn origin(&self, oid: u32, attnum: i16) -> Option<(String, String)> {
        if let Some(hit) = self.origins.borrow().get(&(oid, attnum)) {
            return hit.clone();
        }
        let found = {
            let mut conn = self.conn.borrow_mut();
            conn.client().ok().and_then(|c| {
                c.query_opt(
                    "SELECT c.relname::text, a.attname::text FROM pg_class c JOIN pg_attribute a ON a.attrelid = c.oid \
                     WHERE c.oid = $1 AND a.attnum = $2",
                    &[&oid, &attnum],
                )
                .ok()
                .flatten()
                .map(|r| (r.get::<_, String>(0), r.get::<_, String>(1)))
            })
        };
        self.origins
            .borrow_mut()
            .insert((oid, attnum), found.clone());
        found
    }

    fn control(&self, sql: &str) -> Result<(), DbError> {
        let mut conn = self.conn.borrow_mut();
        let engine = conn.engine().to_string();
        let result = conn
            .client()?
            .batch_execute(sql)
            .map_err(|e| crate::error::map_error(&engine, e));
        drop(conn);
        self.after_error(result)
    }

    fn after_error<T>(&self, result: Result<T, DbError>) -> Result<T, DbError> {
        let result = result.map_err(|e| match e {
            // MySQL's wording, which WordPress and plugins match on.
            DbError::Engine {
                engine,
                code: Some(code),
                message,
            } if code == "42P01" => {
                self.dialect.invalidate_schema();
                let table = message.split('"').nth(1).unwrap_or_default().to_string();
                let database = self.session.borrow().database().to_string();
                DbError::Engine {
                    engine,
                    code: Some("1146".into()),
                    message: format!("Table '{database}.{table}' doesn't exist"),
                }
            }
            other => other,
        });
        if let Err(e) = &result {
            match e {
                DbError::Connection { .. } => {
                    self.conn.borrow_mut().mark_lost();
                    self.in_tx.set(false);
                }
                DbError::Engine { code: Some(c), .. } if c == "42703" || c == "42P01" => {
                    self.dialect.invalidate_schema()
                }
                _ => {}
            }
        }
        result
    }

    fn reopen(&self) -> Result<(), DbError> {
        if self.autocommit_off.get() && !self.in_tx.get() {
            self.control("BEGIN")?;
            self.in_tx.set(true);
        }
        Ok(())
    }

    fn set_autocommit(&self, on: bool) -> Result<(), DbError> {
        self.autocommit_off.set(!on);
        if on && self.in_tx.get() {
            self.control("COMMIT")?;
            self.in_tx.set(false);
        }
        self.reopen()
    }

    /// Runs one emitted statement: its rows (for the caller or the store), and how much it counts.
    fn step(
        &self,
        e: &PgEmitted,
        exec: &mut ExecResult,
        ids: &mut Vec<u64>,
    ) -> Result<Option<Rows>, DbError> {
        let mut conn = self.conn.borrow_mut();
        let engine = conn.engine().to_string();
        let result = match e.count {
            RowCount::Affected => exec::execute(&mut conn, &engine, &e.sql).map(|n| {
                exec.rows_affected += n;
                None
            }),
            _ => exec::query_with_origins(&mut conn, &engine, &e.sql).map(Some),
        };
        drop(conn);
        let rows = self.after_error(result)?;
        let Some((mut rows, origins)) = rows else {
            return Ok(None);
        };
        if e.count == RowCount::Rows {
            crate::origin::describe(
                &mut rows,
                &origins,
                &|oid, attnum| self.origin(oid, attnum),
                &|t| self.introspector.catalogue(t).ok().flatten(),
            );
        }
        let column = |name: &str| rows.columns.iter().position(|c| c.name == name);
        match e.count {
            RowCount::Returning | RowCount::Upsert => {
                let (id, inserted) = (column(ID_ALIAS), column(INSERTED_ALIAS));
                for row in &rows.rows {
                    if let Some(i) = id
                        && let Some(n) = row.get_index(i).and_then(int)
                    {
                        ids.push(n as u64);
                    }
                    exec.rows_affected +=
                        match inserted.and_then(|i| row.get_index(i)).and_then(int) {
                            Some(0) => 2,
                            _ => 1,
                        };
                }
                Ok(None)
            }
            RowCount::CountColumn => {
                exec.rows_affected += rows
                    .rows
                    .first()
                    .and_then(|r| r.get_index(0))
                    .and_then(int)
                    .unwrap_or(0) as u64;
                Ok(None)
            }
            RowCount::FoundRows => {
                let n = rows
                    .rows
                    .first()
                    .and_then(|r| r.get_index(0))
                    .and_then(int)
                    .unwrap_or(0);
                self.found_rows.set(n as u64);
                Ok(None)
            }
            RowCount::Hidden => Ok(None),
            RowCount::Rows | RowCount::Affected => Ok(Some(rows)),
        }
    }

    fn run(&self, stmt: &Statement) -> Result<(Rows, ExecResult), DbError> {
        if let Some(on) = autocommit(stmt) {
            self.set_autocommit(on)?;
            return Ok((Rows::default(), ExecResult::default()));
        }
        if is_last_insert_id(stmt) {
            return Ok((
                one_value("LAST_INSERT_ID()", DbValue::Int(self.last_id.get() as i64)),
                ExecResult::default(),
            ));
        }
        let kind = stmt.kind();
        let begins = kind == StatementKind::Transaction(TxControl::Begin);
        // MySQL commits the open transaction before DDL and before a new START TRANSACTION.
        if (commits_implicitly(stmt) || begins) && self.in_tx.get() {
            self.control("COMMIT")?;
            self.in_tx.set(false);
        }
        let plan = match self.dialect.plan(stmt) {
            Ok(p) => p,
            Err(e) => {
                self.reopen()?;
                return Err(e);
            }
        };
        let emitted = match plan {
            PgPlan::Emulated(EmulatedOp::SetSession { name, value }) if name == "database" => {
                let database = self.session.borrow().database().to_string();
                if !value.eq_ignore_ascii_case(&database) {
                    return Err(DbError::Engine {
                        engine: self.name.clone(),
                        code: Some("1049".into()),
                        message: format!("Unknown database '{value}'"),
                    });
                }
                return Ok((Rows::default(), ExecResult::default()));
            }
            PgPlan::Emulated(op) => {
                let rows = {
                    let mut session = self.session.borrow_mut();
                    answer_emulated(
                        &op,
                        &*self.introspector,
                        &mut session,
                        self.found_rows.get(),
                    )?
                };
                if matches!(op, EmulatedOp::SetSession { .. }) {
                    self.sync_session()?;
                }
                return Ok((rows, ExecResult::default()));
            }
            PgPlan::Sql(emitted) => emitted,
        };
        let transaction_control = matches!(kind, StatementKind::Transaction(_));
        let several = emitted
            .iter()
            .filter(|e| e.count != RowCount::FoundRows)
            .count()
            > 1;
        let (open, close, undo) = if transaction_control {
            (None, None, None)
        } else if self.in_tx.get() {
            (
                Some("SAVEPOINT speedyspec_stmt"),
                Some("RELEASE SAVEPOINT speedyspec_stmt"),
                Some("ROLLBACK TO SAVEPOINT speedyspec_stmt; RELEASE SAVEPOINT speedyspec_stmt"),
            )
        } else if several {
            (Some("BEGIN"), Some("COMMIT"), Some("ROLLBACK"))
        } else {
            (None, None, None)
        };
        if let Some(sql) = open {
            self.control(sql)?;
        }
        let mut exec = ExecResult::default();
        let mut ids = Vec::new();
        let mut rows = None;
        let mut failure = None;
        for e in &emitted {
            match self.step(e, &mut exec, &mut ids) {
                Ok(Some(r)) => rows = Some(r),
                Ok(None) => {}
                Err(err) => {
                    failure = Some(err);
                    break;
                }
            }
        }
        if let Some(err) = failure {
            if let Some(sql) = undo
                && !matches!(err, DbError::Connection { .. })
            {
                let _ = self.control(sql);
            }
            if commits_implicitly(stmt) {
                self.in_tx.set(false);
            }
            self.reopen()?;
            return Err(err);
        }
        if let Some(sql) = close {
            self.control(sql)?;
        }
        if transaction_control {
            match kind {
                StatementKind::Transaction(TxControl::Begin) => self.in_tx.set(true),
                StatementKind::Transaction(TxControl::Commit | TxControl::Rollback) => {
                    self.in_tx.set(false)
                }
                _ => {}
            }
            self.reopen()?;
        }
        if kind == StatementKind::Ddl {
            self.introspector.track_temporary(stmt)?;
            self.dialect.invalidate_schema();
            self.origins.borrow_mut().clear();
            self.conn.borrow_mut().forget_statements();
            let _ = self.control("NOTIFY speedyspec_schema");
        }
        exec.insert_id = ids.into_iter().filter(|i| *i > 0).min();
        if let Some(id) = exec.insert_id {
            self.last_id.set(id);
        }
        let returns_rows = emitted.iter().any(|e| {
            e.count == RowCount::Rows
                && matches!(
                    kind,
                    StatementKind::Select | StatementKind::Show | StatementKind::Other
                )
        });
        let rows = rows.unwrap_or_default();
        if returns_rows {
            if !emitted.iter().any(|e| e.count == RowCount::FoundRows) {
                self.found_rows.set(rows.rows.len() as u64);
            }
            exec.rows_affected = rows.rows.len() as u64;
        }
        Ok((rows, exec))
    }
}

impl SqlStore for PostgresStore {
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
            atomic_increment: false,
            returning: true,
            upsert: UpsertForm::OnConflict,
        }
    }

    fn execute(&self, stmt: &Statement) -> Result<ExecResult, DbError> {
        self.run(stmt).map(|(_, e)| e)
    }

    fn query(&self, stmt: &Statement) -> Result<Rows, DbError> {
        self.run(stmt).map(|(r, _)| r)
    }

    fn begin(&self) -> Result<(), DbError> {
        if self.in_tx.get() {
            self.control("COMMIT")?;
        }
        self.control("BEGIN")?;
        self.in_tx.set(true);
        Ok(())
    }

    fn commit(&self) -> Result<(), DbError> {
        if self.in_tx.get() {
            self.control("COMMIT")?;
            self.in_tx.set(false);
        }
        self.reopen()
    }

    fn rollback(&self) -> Result<(), DbError> {
        if self.in_tx.get() {
            self.control("ROLLBACK")?;
            self.in_tx.set(false);
        }
        self.reopen()
    }

    fn savepoint(&self, name: &str) -> Result<(), DbError> {
        self.control(&format!("SAVEPOINT \"{}\"", name.replace('"', "\"\"")))
    }

    fn release_savepoint(&self, name: &str) -> Result<(), DbError> {
        self.control(&format!(
            "RELEASE SAVEPOINT \"{}\"",
            name.replace('"', "\"\"")
        ))
    }

    fn rollback_to_savepoint(&self, name: &str) -> Result<(), DbError> {
        self.control(&format!(
            "ROLLBACK TO SAVEPOINT \"{}\"",
            name.replace('"', "\"\"")
        ))
    }

    fn in_transaction(&self) -> bool {
        self.in_tx.get()
    }

    fn introspect(&self) -> &dyn Introspector {
        &*self.introspector
    }

    fn reset(&self) -> Result<(), DbError> {
        self.autocommit_off.set(false);
        if self.in_tx.get() {
            let _ = self.control("ROLLBACK");
            self.in_tx.set(false);
        }
        let notified = self.conn.borrow_mut().reset_session();
        match notified {
            Ok(true) => self.dialect.invalidate_schema(),
            Ok(false) => {}
            Err(_) => self.conn.borrow_mut().mark_lost(),
        }
        self.introspector.forget_temporary();
        self.session.borrow_mut().reset();
        self.found_rows.set(0);
        self.last_id.set(0);
        self.sync_session()
    }
}
