//! The MySQL SQL Store (Adapter over the blocking `mysql` driver).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use mysql::consts::ColumnType;
use mysql::prelude::Queryable;
use mysql::{Column, Conn, Opts, Params, Value};
use speedyspec_data::{
    Capabilities, ColumnInfo, DbError, DbValue, Dialect, ExecResult, Introspector, Prepared, Row,
    Rows, SqlStore, Statement, StatementKind, TxControl, UpsertForm,
};
use speedyspec_data_query::parse_one;

use crate::MySqlDialect;

/// One MySQL connection for one worker; it reconnects after a lost connection.
pub struct MySqlStore {
    engine: String,
    opts: Opts,
    init: Vec<Statement>,
    conn: RefCell<Option<Conn>>,
    in_tx: Cell<bool>,
}

impl MySqlStore {
    /// Connects and runs the init statements (`SET NAMES ..`).
    pub fn connect(engine: &str, opts: Opts, init: Vec<Statement>) -> Result<MySqlStore, DbError> {
        let store = MySqlStore {
            engine: engine.into(),
            opts,
            init,
            conn: RefCell::new(None),
            in_tx: Cell::new(false),
        };
        store.ensure()?;
        Ok(store)
    }

    fn ensure(&self) -> Result<(), DbError> {
        if self.conn.borrow().is_some() {
            return Ok(());
        }
        let conn = Conn::new(self.opts.clone()).map_err(|e| self.map(e))?;
        *self.conn.borrow_mut() = Some(conn);
        for stmt in &self.init.clone() {
            self.run(stmt)?;
        }
        Ok(())
    }

    fn map(&self, error: mysql::Error) -> DbError {
        let engine = self.engine.clone();
        match error {
            mysql::Error::MySqlError(e) => match e.code {
                1022 | 1048 | 1062 | 1169 | 1216 | 1217 | 1364 | 1451 | 1452 | 1557 | 1586
                | 3819 => DbError::Constraint {
                    engine,
                    message: e.message,
                },
                1213 => DbError::Deadlock {
                    engine,
                    message: e.message,
                },
                1053 | 1927 | 2006 | 2013 | 4031 => DbError::Connection {
                    engine,
                    message: e.message,
                },
                code => DbError::Engine {
                    engine,
                    code: Some(code.to_string()),
                    message: e.message,
                },
            },
            mysql::Error::DriverError(
                e @ (mysql::DriverError::Params(_)
                | mysql::DriverError::MixedParams
                | mysql::DriverError::MismatchedStmtParams(..)),
            ) => DbError::Engine {
                engine,
                code: None,
                message: e.to_string(),
            },
            mysql::Error::IoError(e) => DbError::Connection {
                engine,
                message: e.to_string(),
            },
            mysql::Error::CodecError(e) => DbError::Connection {
                engine,
                message: e.to_string(),
            },
            mysql::Error::DriverError(e) => DbError::Connection {
                engine,
                message: e.to_string(),
            },
            other => DbError::Engine {
                engine,
                code: None,
                message: other.to_string(),
            },
        }
    }

    /// Runs one statement and collects its first result set.
    pub(crate) fn run(&self, stmt: &Statement) -> Result<(Rows, ExecResult), DbError> {
        self.ensure()?;
        let emitted = match self.dialect().prepare(stmt)? {
            Prepared::Sql(mut sql) if sql.len() == 1 => sql.remove(0),
            other => {
                return Err(DbError::Unsupported {
                    construct: format!("{other:?}"),
                    dialect: speedyspec_data::DialectKind::MySql,
                });
            }
        };
        let result = {
            let mut guard = self.conn.borrow_mut();
            let conn = guard.as_mut().expect("ensured above");
            if emitted.params.is_empty() {
                conn.query_iter(&emitted.sql).and_then(collect)
            } else {
                let params = Params::Positional(emitted.params.iter().map(to_mysql).collect());
                conn.exec_iter(&emitted.sql, params).and_then(collect)
            }
        };
        match result {
            Ok(out) => {
                if let Some(open) = transaction_effect(stmt) {
                    self.in_tx.set(open);
                }
                Ok(out)
            }
            Err(e) => {
                let error = self.map(e);
                match &error {
                    DbError::Connection { .. } => {
                        *self.conn.borrow_mut() = None;
                        self.in_tx.set(false);
                    }
                    // A deadlock rolls the whole transaction back; DDL commits before it runs, so a failed one has
                    // committed too.
                    DbError::Deadlock { .. } => self.in_tx.set(false),
                    _ if transaction_effect(stmt) == Some(false)
                        && stmt.kind() == StatementKind::Ddl =>
                    {
                        self.in_tx.set(false)
                    }
                    _ => {}
                }
                Err(error)
            }
        }
    }

    fn control(&self, sql: &str) -> Result<(), DbError> {
        self.run(&parse_one(sql)?).map(|_| ())
    }
}

/// Whether a statement that ran leaves a transaction open (`Some(true)`), ends it (`Some(false)`, also MySQL's implicit
/// commits) or changes nothing (`None`).
fn transaction_effect(stmt: &Statement) -> Option<bool> {
    use speedyspec_data::ast::{self, Set, Statement as S};
    match stmt.kind() {
        StatementKind::Transaction(TxControl::Begin) => Some(true),
        StatementKind::Transaction(TxControl::Commit | TxControl::Rollback) => Some(false),
        // Temporary tables are the DDL MySQL does not commit for.
        StatementKind::Ddl => match stmt.tree() {
            S::CreateTable(create) if create.temporary => None,
            S::Drop {
                temporary: true, ..
            } => None,
            _ => Some(false),
        },
        StatementKind::Set => {
            let autocommit = |name: &ast::ObjectName, values: &[ast::Expr]| -> Option<bool> {
                let name = name.to_string().to_ascii_lowercase();
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
                    "0" | "off" | "false" => Some(true),
                    "1" | "on" | "true" => Some(false),
                    _ => None,
                }
            };
            match stmt.tree() {
                S::Set(Set::SingleAssignment {
                    variable, values, ..
                }) => autocommit(variable, values),
                S::Set(Set::MultipleAssignments { assignments }) => assignments
                    .iter()
                    .filter_map(|a| autocommit(&a.name, std::slice::from_ref(&a.value)))
                    .next_back(),
                _ => None,
            }
        }
        _ => match stmt.tree() {
            S::LockTables { .. } | S::Lock(_) => Some(false),
            _ => None,
        },
    }
}

/// A double as MySQL's text protocol prints it: fixed notation for decimal exponents -15 to 14, else `1e20`,
/// `1.5e-16`; `digits` and `exp` are the shortest round-trip digits and the leading digit's exponent.
fn mysql_float_text(negative: bool, digits: &str, exp: i32) -> String {
    let sign = if negative { "-" } else { "" };
    let n = digits.len() as i32;
    if (-15..15).contains(&exp) {
        if exp >= 0 {
            if n > exp + 1 {
                format!(
                    "{sign}{}.{}",
                    &digits[..(exp + 1) as usize],
                    &digits[(exp + 1) as usize..]
                )
            } else {
                format!("{sign}{digits}{}", "0".repeat((exp + 1 - n) as usize))
            }
        } else {
            format!("{sign}0.{}{digits}", "0".repeat((-exp - 1) as usize))
        }
    } else {
        let frac = if n > 1 {
            format!(".{}", &digits[1..])
        } else {
            String::new()
        };
        format!("{sign}{}{frac}e{exp}", &digits[..1])
    }
}

fn float_text(sci: String, negative: bool) -> String {
    if sci == "0e0" {
        return if negative { "-0".into() } else { "0".into() };
    }
    let (mantissa, exp) = sci.split_once('e').expect("{:e} has an exponent");
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    mysql_float_text(
        negative,
        &digits,
        exp.parse().expect("exponent is an integer"),
    )
}

fn collect<T: mysql::prelude::Protocol>(
    mut result: mysql::QueryResult<'_, '_, '_, T>,
) -> mysql::Result<(Rows, ExecResult)> {
    let mut rows = Rows::default();
    // The OK packet's counts are read before the result set is consumed; afterwards they describe the next set.
    let exec = ExecResult {
        rows_affected: result.affected_rows(),
        insert_id: result.last_insert_id().filter(|id| *id != 0),
    };
    if let Some(set) = result.iter() {
        let columns: Rc<[Column]> = Rc::from(set.columns().as_ref().to_vec());
        let names: Rc<[String]> = columns.iter().map(|c| c.name_str().into_owned()).collect();
        let mut max = vec![0u64; columns.len()];
        for row in set {
            let row = row?;
            let values: Vec<DbValue> = row
                .unwrap()
                .into_iter()
                .zip(columns.iter())
                .map(|(v, c)| from_mysql(v, c))
                .collect();
            for (m, v) in max.iter_mut().zip(&values) {
                *m = (*m).max(v.to_mysql_text().map_or(0, |t| t.len() as u64));
            }
            rows.rows.push(Row::new(names.clone(), values));
        }
        rows.columns = columns
            .iter()
            .zip(max)
            .map(|(c, max_length)| column_info(c, max_length))
            .collect();
    }
    // Drain any further result sets so the connection stays usable.
    while result.iter().is_some() {
        for row in result.iter().into_iter().flatten() {
            row?;
        }
    }
    Ok((rows, exec))
}

/// mysqlnd's `NUM_FLAG`, which the client sets on numeric columns (the server never sends it).
const NUM_FLAG: u32 = 32768;

/// mysqlnd's `IS_NUM`: the integer and floating types up to `INT24` (but not `TIMESTAMP`), `YEAR` and `NEWDECIMAL`.
fn is_numeric(t: ColumnType) -> bool {
    use ColumnType::*;
    matches!(
        t,
        MYSQL_TYPE_DECIMAL
            | MYSQL_TYPE_TINY
            | MYSQL_TYPE_SHORT
            | MYSQL_TYPE_LONG
            | MYSQL_TYPE_FLOAT
            | MYSQL_TYPE_DOUBLE
            | MYSQL_TYPE_NULL
            | MYSQL_TYPE_LONGLONG
            | MYSQL_TYPE_INT24
            | MYSQL_TYPE_YEAR
            | MYSQL_TYPE_NEWDECIMAL
    )
}

fn column_info(c: &Column, max_length: u64) -> ColumnInfo {
    ColumnInfo {
        name: c.name_str().into_owned(),
        org_name: c.org_name_str().into_owned(),
        table: c.table_str().into_owned(),
        org_table: c.org_table_str().into_owned(),
        sql_type: format!("{:?}", c.column_type()),
        length: u64::from(c.column_length()),
        max_length,
        nullable: !c
            .flags()
            .contains(mysql::consts::ColumnFlags::NOT_NULL_FLAG),
        flags: c.flags().bits() as u32
            | if is_numeric(c.column_type()) {
                NUM_FLAG
            } else {
                0
            },
        decimals: c.decimals(),
        charset: c.character_set(),
    }
}

fn to_mysql(v: &DbValue) -> Value {
    match v {
        DbValue::Null => Value::NULL,
        DbValue::Int(i) => Value::Int(*i),
        DbValue::UInt(u) => Value::UInt(*u),
        DbValue::Float(f) => Value::Double(*f),
        DbValue::Text(s) => Value::Bytes(s.as_bytes().to_vec()),
        DbValue::Bytes(b) => Value::Bytes(b.clone()),
    }
}

fn from_mysql(v: Value, column: &Column) -> DbValue {
    match v {
        Value::NULL => DbValue::Null,
        Value::Bytes(b) => match String::from_utf8(b) {
            Ok(s) => DbValue::Text(s),
            Err(e) => DbValue::Bytes(e.into_bytes()),
        },
        // The binary protocol's typed values become the text the text protocol gives, so a result is the same
        // whether or not the statement had params.
        Value::Int(i) => DbValue::Text(i.to_string()),
        Value::UInt(u) => DbValue::Text(u.to_string()),
        Value::Float(f) => {
            DbValue::Text(float_text(format!("{:e}", f.abs()), f.is_sign_negative()))
        }
        Value::Double(d) => {
            DbValue::Text(float_text(format!("{:e}", d.abs()), d.is_sign_negative()))
        }
        Value::Date(y, mo, d, h, mi, s, us) => {
            let date = format!("{y:04}-{mo:02}-{d:02}");
            if column.column_type() == ColumnType::MYSQL_TYPE_DATE {
                DbValue::Text(date)
            } else {
                DbValue::Text(format!(
                    "{date} {h:02}:{mi:02}:{s:02}{}",
                    fraction(us, column.decimals())
                ))
            }
        }
        Value::Time(neg, days, h, mi, s, us) => {
            let hours = u64::from(days) * 24 + u64::from(h);
            let sign = if neg { "-" } else { "" };
            DbValue::Text(format!(
                "{sign}{hours:02}:{mi:02}:{s:02}{}",
                fraction(us, column.decimals())
            ))
        }
    }
}

fn fraction(us: u32, decimals: u8) -> String {
    let digits = usize::from(decimals.min(6));
    if digits == 0 {
        String::new()
    } else {
        format!(".{:06}", us)[..digits + 1].to_string()
    }
}

impl SqlStore for MySqlStore {
    fn engine(&self) -> &str {
        &self.engine
    }

    fn dialect(&self) -> &dyn Dialect {
        &MySqlDialect
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
        self.run(stmt).map(|(_, exec)| exec)
    }

    fn query(&self, stmt: &Statement) -> Result<Rows, DbError> {
        self.run(stmt).map(|(rows, _)| rows)
    }

    fn begin(&self) -> Result<(), DbError> {
        self.control("START TRANSACTION")
    }

    fn commit(&self) -> Result<(), DbError> {
        self.control("COMMIT")
    }

    fn rollback(&self) -> Result<(), DbError> {
        self.control("ROLLBACK")
    }

    fn savepoint(&self, name: &str) -> Result<(), DbError> {
        self.control(&format!("SAVEPOINT {}", quote(name)))
    }

    fn release_savepoint(&self, name: &str) -> Result<(), DbError> {
        self.control(&format!("RELEASE SAVEPOINT {}", quote(name)))
    }

    fn rollback_to_savepoint(&self, name: &str) -> Result<(), DbError> {
        self.control(&format!("ROLLBACK TO SAVEPOINT {}", quote(name)))
    }

    fn in_transaction(&self) -> bool {
        self.in_tx.get()
    }

    fn introspect(&self) -> &dyn Introspector {
        self
    }

    fn reset(&self) -> Result<(), DbError> {
        if self.conn.borrow().is_none() {
            self.in_tx.set(false);
            return Ok(());
        }
        if self.in_tx.get() {
            let _ = self.rollback();
        }
        self.in_tx.set(false);
        let reset = self.conn.borrow_mut().as_mut().map(|c| c.reset());
        if let Some(Err(e)) = reset {
            *self.conn.borrow_mut() = None;
            return Err(self.map(e));
        }
        for stmt in &self.init.clone() {
            self.run(stmt)?;
        }
        Ok(())
    }
}

/// A backquoted identifier.
pub(crate) fn quote(name: &str) -> String {
    format!("`{}`", name.replace('`', "``"))
}
