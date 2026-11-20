//! The ReDB engine (Abstract Factory) for `redb:///absolute/file.redb` URLs. It never opens the file: only the
//! owner does; workers get a `RedbClient` on the owner's socket.

use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use speedyspec_data::{DbError, Engine, EngineConfig, EngineFactory, EngineHandle, EngineKind};

use crate::client::RedbClient;
use crate::owner::OwnerConfig;

#[derive(Debug, Clone, Copy, Default)]
pub struct RedbEngineFactory;

impl EngineFactory for RedbEngineFactory {
    fn scheme(&self) -> &'static str {
        "redb"
    }

    fn create(&self, config: EngineConfig) -> Result<Rc<dyn Engine>, DbError> {
        // The owner's reading of the URL and `socket` option, so client and owner agree on the socket.
        let owner = OwnerConfig::from_engine_config(&config)?;
        let timeout = match config.options.get("timeout_ms") {
            Some(v) => Duration::from_millis(v.parse().map_err(|_| {
                DbError::Config(format!(
                    "engine `{}`: timeout_ms `{v}` is not a number",
                    config.name
                ))
            })?),
            None => Duration::from_millis(5000),
        };
        RedbClient::new(&config.name, owner.socket.clone(), timeout)?;
        Ok(Rc::new(RedbEngine {
            name: config.name,
            socket: owner.socket,
            timeout,
        }))
    }
}

struct RedbEngine {
    name: String,
    socket: PathBuf,
    timeout: Duration,
}

impl Engine for RedbEngine {
    fn name(&self) -> &str {
        &self.name
    }

    fn kind(&self) -> EngineKind {
        EngineKind::Redb
    }

    fn open(&self) -> Result<EngineHandle, DbError> {
        Ok(EngineHandle::NoSql(Rc::new(RedbClient::new(
            &self.name,
            self.socket.clone(),
            self.timeout,
        )?)))
    }
}
