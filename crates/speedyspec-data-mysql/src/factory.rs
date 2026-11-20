//! The MySQL engine (Abstract Factory) and its factory for `mysql://` URLs.

use std::rc::Rc;

use mysql::{Opts, OptsBuilder};
use speedyspec_data::{DbError, Engine, EngineConfig, EngineFactory, EngineHandle, EngineKind};
use speedyspec_data_query::parse_one;

use crate::MySqlStore;

/// Builds MySQL engines from `mysql://user:password@host[:port]/database`, with options `socket`, `charset`
/// (default `utf8mb4`) and `collate` (default: the server's).
#[derive(Debug, Clone, Copy, Default)]
pub struct MySqlEngineFactory;

impl EngineFactory for MySqlEngineFactory {
    fn scheme(&self) -> &'static str {
        "mysql"
    }

    fn create(&self, config: EngineConfig) -> Result<Rc<dyn Engine>, DbError> {
        let opts = Opts::from_url(&config.url)
            .map_err(|e| DbError::Config(format!("engine `{}`: {e}", config.name)))?;
        let mut builder = OptsBuilder::from_opts(opts);
        if let Some(socket) = config.options.get("socket") {
            builder = builder.socket(Some(socket.clone()));
        }
        let charset = config
            .options
            .get("charset")
            .map_or("utf8mb4", String::as_str);
        let names = match config.options.get("collate").filter(|c| !c.is_empty()) {
            Some(collate) => format!(
                "SET NAMES '{}' COLLATE '{}'",
                literal(charset),
                literal(collate)
            ),
            None => format!("SET NAMES '{}'", literal(charset)),
        };
        parse_one(&names)?;
        Ok(Rc::new(MySqlEngine {
            name: config.name,
            opts: builder.into(),
            names,
        }))
    }
}

fn literal(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "''")
}

struct MySqlEngine {
    name: String,
    opts: Opts,
    names: String,
}

impl Engine for MySqlEngine {
    fn name(&self) -> &str {
        &self.name
    }

    fn kind(&self) -> EngineKind {
        EngineKind::MySql
    }

    fn open(&self) -> Result<EngineHandle, DbError> {
        let store =
            MySqlStore::connect(&self.name, self.opts.clone(), vec![parse_one(&self.names)?])?;
        Ok(EngineHandle::Sql(Rc::new(store)))
    }
}
