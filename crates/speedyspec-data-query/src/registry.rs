//! The engine registry (Registry): configured engines, routes from uses to engines, and opened handles.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use indexmap::IndexMap;
use speedyspec_data::{
    DbError, DirectStore, Engine, EngineConfig, EngineFactory, EngineHandle, EngineLookup,
    NoSqlStore, SqlStore,
};

/// Engines for one worker; a handle opens once and lives for the worker's life.
#[derive(Default)]
pub struct EngineRegistry {
    factories: RefCell<HashMap<&'static str, Rc<dyn EngineFactory>>>,
    engines: RefCell<IndexMap<String, Rc<dyn Engine>>>,
    routes: RefCell<HashMap<String, String>>,
    handles: RefCell<HashMap<String, EngineHandle>>,
}

impl EngineRegistry {
    pub fn new() -> EngineRegistry {
        EngineRegistry::default()
    }

    pub fn register_factory(&self, factory: Rc<dyn EngineFactory>) {
        self.factories
            .borrow_mut()
            .insert(factory.scheme(), factory);
    }

    /// Adds an engine; its URL's scheme picks the factory.
    pub fn configure(&self, config: EngineConfig) -> Result<(), DbError> {
        let scheme = config
            .url
            .split_once("://")
            .map(|(scheme, _)| scheme)
            .ok_or_else(|| {
                DbError::Config(format!("engine `{}` has a URL with no scheme", config.name))
            })?;
        if self.engines.borrow().contains_key(&config.name) {
            return Err(DbError::Config(format!(
                "engine `{}` is configured twice",
                config.name
            )));
        }
        let factory = self
            .factories
            .borrow()
            .get(scheme)
            .cloned()
            .ok_or_else(|| DbError::Config(format!("no engine factory for scheme `{scheme}`")))?;
        let name = config.name.clone();
        let engine = factory.create(config)?;
        self.engines.borrow_mut().insert(name, engine);
        Ok(())
    }

    /// Sends one use (e.g. `object-cache`) to a configured engine.
    pub fn route(&self, use_name: &str, engine: &str) -> Result<(), DbError> {
        if !self.engines.borrow().contains_key(engine) {
            return Err(DbError::NoEngine(engine.into()));
        }
        self.routes
            .borrow_mut()
            .insert(use_name.into(), engine.into());
        Ok(())
    }

    /// The engine a use goes to: its route, else `default`.
    pub fn engine_for(&self, use_name: &str) -> String {
        self.routes
            .borrow()
            .get(use_name)
            .cloned()
            .unwrap_or_else(|| "default".into())
    }

    pub fn sql(&self, use_name: &str) -> Result<Rc<dyn SqlStore>, DbError> {
        let engine = self.engine_for(use_name);
        self.handle(&engine)?
            .as_sql()
            .cloned()
            .ok_or(DbError::WrongKind {
                engine,
                expected: "sql",
            })
    }

    pub fn nosql(&self, use_name: &str) -> Result<Rc<dyn NoSqlStore>, DbError> {
        let engine = self.engine_for(use_name);
        self.handle(&engine)?
            .as_nosql()
            .cloned()
            .ok_or(DbError::WrongKind {
                engine,
                expected: "nosql",
            })
    }

    pub fn direct(&self, use_name: &str) -> Result<Rc<dyn DirectStore>, DbError> {
        let engine = self.engine_for(use_name);
        self.handle(&engine)?
            .as_direct()
            .cloned()
            .ok_or(DbError::WrongKind {
                engine,
                expected: "direct",
            })
    }

    /// Resets every opened SQL store; the runtime calls it at request end. Every store is reset; the first error
    /// is returned.
    pub fn reset_all(&self) -> Result<(), DbError> {
        let stores: Vec<Rc<dyn SqlStore>> = self
            .handles
            .borrow()
            .values()
            .filter_map(|h| h.as_sql().cloned())
            .collect();
        let mut first = None;
        for store in stores {
            if let Err(e) = store.reset() {
                first.get_or_insert(e);
            }
        }
        first.map_or(Ok(()), Err)
    }
}

impl EngineLookup for EngineRegistry {
    fn handle(&self, engine: &str) -> Result<EngineHandle, DbError> {
        if let Some(handle) = self.handles.borrow().get(engine) {
            return Ok(handle.clone());
        }
        let opened = self
            .engines
            .borrow()
            .get(engine)
            .cloned()
            .ok_or_else(|| DbError::NoEngine(engine.into()))?;
        let handle = opened.open()?;
        self.handles
            .borrow_mut()
            .insert(engine.into(), handle.clone());
        Ok(handle)
    }

    fn commit_order(&self) -> Vec<String> {
        self.engines.borrow().keys().cloned().collect()
    }
}
