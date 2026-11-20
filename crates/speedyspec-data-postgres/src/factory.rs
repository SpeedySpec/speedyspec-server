//! The PostgreSQL engine (Abstract Factory) and its factory for `postgres://` URLs.

use std::rc::Rc;
use std::str::FromStr;

use speedyspec_data::{DbError, Engine, EngineConfig, EngineFactory, EngineHandle, EngineKind};

use crate::PostgresStore;

/// Builds PostgreSQL engines from `postgres://user:password@host[:port]/database`, with options `schema` (default
/// `public`), `application_name` (default `speedyspec`) and `sslmode` (only `disable`).
#[derive(Debug, Clone, Copy, Default)]
pub struct PostgresEngineFactory;

impl EngineFactory for PostgresEngineFactory {
    fn scheme(&self) -> &'static str {
        "postgres"
    }

    fn create(&self, config: EngineConfig) -> Result<Rc<dyn Engine>, DbError> {
        postgres::Config::from_str(&config.url)
            .map_err(|e| DbError::Config(format!("engine `{}`: {e}", config.name)))?;
        if config
            .options
            .get("sslmode")
            .is_some_and(|m| m != "disable")
        {
            return Err(DbError::Config(
                "TLS for postgres is not supported yet".into(),
            ));
        }
        Ok(Rc::new(PostgresEngine { config }))
    }
}

pub struct PostgresEngine {
    config: EngineConfig,
}

impl Engine for PostgresEngine {
    fn name(&self) -> &str {
        &self.config.name
    }

    fn kind(&self) -> EngineKind {
        EngineKind::Postgres
    }

    fn open(&self) -> Result<EngineHandle, DbError> {
        Ok(EngineHandle::Sql(Rc::new(PostgresStore::open(
            &self.config,
        )?)))
    }
}
