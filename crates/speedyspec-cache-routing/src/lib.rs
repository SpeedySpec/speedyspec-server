//! Choosing the persistent Caching Store (Abstract Factory): the engine the `object-cache` use is routed to decides
//! the backend (Strategy). Unrouted, the object cache stays request-lifetime, as WordPress without a drop-in.

use std::rc::Rc;

use speedyspec_cache::{CacheError, CachingStore};
use speedyspec_cache_file::FileCache;
use speedyspec_cache_kv::KvCache;
use speedyspec_cache_sql::SqlCache;
use speedyspec_data::{DbError, EngineHandle, EngineLookup};
use speedyspec_data_query::EngineRegistry;

/// The use the object cache's second level is routed by.
pub const OBJECT_CACHE_USE: &str = "object-cache";

/// The cache's table on an SQL engine, its keyspace on a NoSQL one and its directory under a file root.
const SQL_TABLE: &str = "wp_speedyspec_cache";
const KEYSPACE: &str = "object-cache";
const DIRECTORY: &str = "object-cache";

#[derive(Debug, Clone)]
pub struct PersistentCacheSelector {
    sql_table: String,
}

impl Default for PersistentCacheSelector {
    fn default() -> Self {
        Self::new()
    }
}

fn config(e: CacheError) -> DbError {
    DbError::Config(e.to_string())
}

impl PersistentCacheSelector {
    pub fn new() -> PersistentCacheSelector {
        PersistentCacheSelector {
            sql_table: SQL_TABLE.to_string(),
        }
    }

    /// The SQL cache table for a site whose table prefix is not `wp_`.
    pub fn with_sql_table(mut self, table: &str) -> PersistentCacheSelector {
        self.sql_table = table.to_string();
        self
    }

    /// The Caching Store for the engine `use_name` is routed to; `None` when it is not routed.
    pub fn select(
        &self,
        registry: &EngineRegistry,
        use_name: &str,
    ) -> Result<Option<Rc<dyn CachingStore>>, DbError> {
        let Some(engine) = registry.routed(use_name) else {
            return Ok(None);
        };
        let store: Rc<dyn CachingStore> = match registry.handle(&engine)? {
            EngineHandle::Sql(sql) => Rc::new(SqlCache::new(sql, &self.sql_table).map_err(config)?),
            EngineHandle::NoSql(kv) => Rc::new(KvCache::new(kv, KEYSPACE)),
            EngineHandle::Direct(direct) if direct.kind() == "file" => {
                Rc::new(FileCache::new(&*direct, DIRECTORY).map_err(config)?)
            }
            EngineHandle::Direct(_) => {
                return Err(DbError::WrongKind {
                    engine,
                    expected: "sql, nosql or file",
                });
            }
        };
        Ok(Some(store))
    }
}
