//! One PostgreSQL connection, set up for MySQL's statements: UTC, ISO dates, the site's schema, the support objects.

use std::str::FromStr;

use postgres::{Client, NoTls};
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
}

fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

impl Connection {
    pub fn new(engine: &str, url: &str, schema: &str) -> Result<Connection, DbError> {
        let config = postgres::Config::from_str(url)
            .map_err(|e| DbError::Config(format!("engine `{engine}`: {e}")))?;
        Ok(Connection {
            engine: engine.to_string(),
            config,
            schema: schema.to_string(),
            client: None,
        })
    }

    #[allow(dead_code)] // the store names its schema in errors (Task 8)
    pub(crate) fn schema(&self) -> &str {
        &self.schema
    }

    pub(crate) fn engine(&self) -> &str {
        &self.engine
    }

    /// The client, connecting on first use or after a loss.
    pub fn client(&mut self) -> Result<&mut Client, DbError> {
        if self.client.as_ref().is_some_and(Client::is_closed) {
            self.client = None;
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
                .batch_execute(&format!(
                    "SET TimeZone = 'UTC'; SET DateStyle = 'ISO, YMD'; SET IntervalStyle = 'postgres'; \
                     SET extra_float_digits = 1; SET standard_conforming_strings = on; SET client_encoding = 'UTF8'; \
                     CREATE SCHEMA IF NOT EXISTS {s}; SET search_path = {s}; LISTEN speedyspec_schema",
                    s = quote(&self.schema)
                ))
                .map_err(|e| map_error(&engine, e))?;
            crate::support::ensure(&mut client, &engine)?;
            self.client = Some(client);
        }
        Ok(self.client.as_mut().expect("connected above"))
    }

    #[allow(dead_code)] // the store drops a lost connection (Task 8)
    pub(crate) fn mark_lost(&mut self) {
        self.client = None;
    }
}
