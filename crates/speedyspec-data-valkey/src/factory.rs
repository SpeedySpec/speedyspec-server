//! The Valkey engine (Abstract Factory) for `valkey://[user:password@]host[:port][/db]` URLs.

use std::rc::Rc;
use std::time::Duration;

use speedyspec_data::{DbError, Engine, EngineConfig, EngineFactory, EngineHandle, EngineKind};

use crate::{ValkeyStore, driver_url};

#[derive(Debug, Clone, Copy, Default)]
pub struct ValkeyEngineFactory;

impl EngineFactory for ValkeyEngineFactory {
    fn scheme(&self) -> &'static str {
        "valkey"
    }

    fn create(&self, config: EngineConfig) -> Result<Rc<dyn Engine>, DbError> {
        let tls = config.options.get("tls").is_some_and(|v| v == "true");
        let url = driver_url(&config.url, tls)?;
        let prefix = config
            .options
            .get("prefix")
            .cloned()
            .unwrap_or_else(|| "speedyspec:".into());
        let timeout = match config.options.get("timeout_ms") {
            Some(v) => Duration::from_millis(v.parse().map_err(|_| {
                DbError::Config(format!(
                    "engine `{}`: timeout_ms `{v}` is not a number",
                    config.name
                ))
            })?),
            None => Duration::from_millis(2000),
        };
        // Validates the URL and prefix now, connects on first use.
        ValkeyStore::new(&config.name, &url, &prefix, timeout)?;
        Ok(Rc::new(ValkeyEngine {
            name: config.name,
            url,
            prefix,
            timeout,
        }))
    }
}

struct ValkeyEngine {
    name: String,
    url: String,
    prefix: String,
    timeout: Duration,
}

impl Engine for ValkeyEngine {
    fn name(&self) -> &str {
        &self.name
    }

    fn kind(&self) -> EngineKind {
        EngineKind::Valkey
    }

    fn open(&self) -> Result<EngineHandle, DbError> {
        Ok(EngineHandle::NoSql(Rc::new(ValkeyStore::new(
            &self.name,
            &self.url,
            &self.prefix,
            self.timeout,
        )?)))
    }
}
