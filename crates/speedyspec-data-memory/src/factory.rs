//! The memory engine (Abstract Factory): `memory://` (or `memory://nosql`) for a NoSQL Store, `memory://direct` for
//! a Direct Store. The engine makes its store once and hands out the same one.

use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_data::{DbError, Engine, EngineConfig, EngineFactory, EngineHandle, EngineKind};

use crate::{MemoryDirectStore, MemoryNoSqlStore};

#[derive(Debug, Clone, Copy, Default)]
pub struct MemoryEngineFactory;

#[derive(Clone, Copy)]
enum Kind {
    NoSql,
    Direct,
}

impl EngineFactory for MemoryEngineFactory {
    fn scheme(&self) -> &'static str {
        "memory"
    }

    fn create(&self, config: EngineConfig) -> Result<Rc<dyn Engine>, DbError> {
        let kind = match config.url.as_str() {
            "memory://" | "memory://nosql" => Kind::NoSql,
            "memory://direct" => Kind::Direct,
            other => {
                return Err(DbError::Config(format!(
                    "engine `{}`: a memory URL is memory://, memory://nosql or memory://direct, not `{other}`",
                    config.name
                )));
            }
        };
        Ok(Rc::new(MemoryEngine {
            name: config.name,
            kind,
            handle: RefCell::new(None),
        }))
    }
}

struct MemoryEngine {
    name: String,
    kind: Kind,
    handle: RefCell<Option<EngineHandle>>,
}

impl Engine for MemoryEngine {
    fn name(&self) -> &str {
        &self.name
    }

    fn kind(&self) -> EngineKind {
        EngineKind::Memory
    }

    fn open(&self) -> Result<EngineHandle, DbError> {
        if let Some(handle) = self.handle.borrow().as_ref() {
            return Ok(handle.clone());
        }
        let handle = match self.kind {
            Kind::NoSql => EngineHandle::NoSql(Rc::new(MemoryNoSqlStore::new(&self.name))),
            Kind::Direct => {
                let store = MemoryDirectStore::new(&self.name);
                speedyspec_data::DirectStore::connect(&store)?;
                EngineHandle::Direct(Rc::new(store))
            }
        };
        *self.handle.borrow_mut() = Some(handle.clone());
        Ok(handle)
    }
}
