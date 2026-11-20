//! One PostgreSQL connection, set up for MySQL's statements: UTC, ISO dates, the site's schema, the support objects.

use std::str::FromStr;

use indexmap::IndexMap;
use postgres::types::Type;
use postgres::{Client, NoTls, Statement};
use speedyspec_data::DbError;

use crate::error::map_error;

/// PostgreSQL 16 adds `any_value`, which MySQL's loose GROUP BY needs.
pub fn check_server_version(num: u32, text: &str) -> Result<(), DbError> {
    if num >= 160_000 {
        Ok(())
    } else {
        Err(DbError::Connection {
            engine: "postgres".into(),
            message: format!("PostgreSQL 16 or newer is required (server is {text})"),
        })
    }
}

pub struct Connection {
    engine: String,
    config: postgres::Config,
    schema: String,
    client: Option<Client>,
    /// Prepared statements by text, oldest evicted first.
    statements: IndexMap<String, Statement>,
}

const CACHED_STATEMENTS: usize = 256;

fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

impl Connection {
    pub fn new(engine: &str, url: &str, schema: &str) -> Result<Connection, DbError> {
        let mut config = postgres::Config::from_str(url)
            .map_err(|e| DbError::Config(format!("engine `{engine}`: {e}")))?;
        if config.get_application_name().is_none() {
            config.application_name("speedyspec");
        }
        Ok(Connection {
            engine: engine.to_string(),
            config,
            schema: schema.to_string(),
            client: None,
            statements: IndexMap::new(),
        })
    }

    pub(crate) fn set_application_name(&mut self, name: &str) {
        self.config.application_name(name);
    }

    /// The database the URL names, which `DATABASE()` reports.
    pub(crate) fn database(&self) -> String {
        self.config.get_dbname().unwrap_or("postgres").to_string()
    }

    fn session_sql(&self) -> String {
        format!(
            "SET TimeZone = 'UTC'; SET DateStyle = 'ISO, YMD'; SET IntervalStyle = 'postgres'; \
             SET extra_float_digits = 1; SET standard_conforming_strings = on; SET client_encoding = 'UTF8'; \
             CREATE SCHEMA IF NOT EXISTS {s}; SET search_path = {s}; LISTEN speedyspec_schema",
            s = quote(&self.schema)
        )
    }

    /// A prepared statement for `sql`, from the cache. A parameter PostgreSQL cannot type from its context (`SELECT
    /// $1`) is typed as text, as MySQL reads it.
    pub(crate) fn prepare(&mut self, sql: &str) -> Result<Statement, DbError> {
        if let Some(statement) = self.statements.get(sql) {
            return Ok(statement.clone());
        }
        let engine = self.engine.clone();
        let client = self.client()?;
        let statement = match client.prepare(sql) {
            Ok(s) => s,
            Err(e) if e.code() == Some(&postgres::error::SqlState::INDETERMINATE_DATATYPE) => {
                let count = sql.matches('$').count();
                client
                    .prepare_typed(sql, &vec![Type::TEXT; count])
                    .map_err(|e| map_error(&engine, e))?
            }
            Err(e) => return Err(map_error(&engine, e)),
        };
        if self.statements.len() >= CACHED_STATEMENTS {
            self.statements.shift_remove_index(0);
        }
        self.statements.insert(sql.to_string(), statement.clone());
        Ok(statement)
    }

    pub(crate) fn forget_statements(&mut self) {
        self.statements.clear();
    }

    /// Back to a fresh session: `DISCARD ALL`, then the setup again; whether a schema change was announced.
    pub(crate) fn reset_session(&mut self) -> Result<bool, DbError> {
        self.statements.clear();
        let Some(client) = self.client.as_mut() else {
            return Ok(false);
        };
        let notified =
            postgres::fallible_iterator::FallibleIterator::count(client.notifications().iter())
                .unwrap_or(0)
                > 0;
        let engine = self.engine.clone();
        let setup = self.session_sql();
        let client = self.client.as_mut().expect("checked above");
        client
            .batch_execute("DISCARD ALL")
            .map_err(|e| map_error(&engine, e))?;
        client
            .batch_execute(&setup)
            .map_err(|e| map_error(&engine, e))?;
        Ok(notified)
    }

    pub(crate) fn engine(&self) -> &str {
        &self.engine
    }

    /// The client, connecting on first use or after a loss.
    pub fn client(&mut self) -> Result<&mut Client, DbError> {
        if self.client.as_ref().is_some_and(Client::is_closed) {
            self.client = None;
            self.statements.clear();
        }
        if self.client.is_none() {
            let engine = self.engine.clone();
            let mut client = self
                .config
                .connect(NoTls)
                .map_err(|e| DbError::Connection {
                    engine: engine.clone(),
                    message: e.to_string(),
                })?;
            let row = client
                .query_one("SELECT current_setting('server_version_num')::int, current_setting('server_version')", &[])
                .map_err(|e| map_error(&engine, e))?;
            let (num, text): (i32, String) = (row.get(0), row.get(1));
            check_server_version(num as u32, &text)?;
            client
                .batch_execute(&self.session_sql())
                .map_err(|e| map_error(&engine, e))?;
            crate::support::ensure(&mut client, &engine)?;
            self.client = Some(client);
        }
        Ok(self.client.as_mut().expect("connected above"))
    }

    pub(crate) fn mark_lost(&mut self) {
        self.client = None;
        self.statements.clear();
    }
}
