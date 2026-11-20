//! The SQLite engine (Abstract Factory) and its factory for `sqlite://` URLs.

use std::rc::Rc;

use speedyspec_data::{DbError, Engine, EngineConfig, EngineFactory, EngineHandle, EngineKind};

use crate::SqliteStore;

/// Builds SQLite engines from `sqlite://:memory:` or `sqlite:///<absolute path>`, with options `busy_timeout_ms`
/// (default 5000) and `database` (the name `DATABASE()` reports; default the file's stem, `main` in memory).
#[derive(Debug, Clone, Copy, Default)]
pub struct SqliteEngineFactory;

impl EngineFactory for SqliteEngineFactory {
    fn scheme(&self) -> &'static str {
        "sqlite"
    }

    fn create(&self, config: EngineConfig) -> Result<Rc<dyn Engine>, DbError> {
        let rest = config.url.strip_prefix("sqlite://").unwrap_or_default();
        if rest != ":memory:" && !rest.starts_with('/') {
            return Err(DbError::Config(format!(
                "engine `{}`: a SQLite URL is sqlite://:memory: or sqlite:///<absolute path>",
                config.name
            )));
        }
        Ok(Rc::new(SqliteEngine { config }))
    }
}

pub struct SqliteEngine {
    config: EngineConfig,
}

impl Engine for SqliteEngine {
    fn name(&self) -> &str {
        &self.config.name
    }

    fn kind(&self) -> EngineKind {
        EngineKind::Sqlite
    }

    fn open(&self) -> Result<EngineHandle, DbError> {
        Ok(EngineHandle::Sql(Rc::new(SqliteStore::open(&self.config)?)))
    }
}
