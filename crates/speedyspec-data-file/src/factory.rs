//! The file engine (Abstract Factory) for `file:///absolute/dir` URLs.

use std::path::PathBuf;
use std::rc::Rc;

use speedyspec_data::{
    DbError, DirectStore, Engine, EngineConfig, EngineFactory, EngineHandle, EngineKind,
};

use crate::FileStore;

#[derive(Debug, Clone, Copy, Default)]
pub struct FileEngineFactory;

impl EngineFactory for FileEngineFactory {
    fn scheme(&self) -> &'static str {
        "file"
    }

    fn create(&self, config: EngineConfig) -> Result<Rc<dyn Engine>, DbError> {
        let path = config.url.strip_prefix("file://").unwrap_or_default();
        if !path.starts_with('/') {
            return Err(DbError::Config(format!(
                "engine `{}`: a file URL is file:///absolute/dir, not `{}`",
                config.name, config.url
            )));
        }
        Ok(Rc::new(FileEngine {
            name: config.name,
            root: PathBuf::from(path),
        }))
    }
}

struct FileEngine {
    name: String,
    root: PathBuf,
}

impl Engine for FileEngine {
    fn name(&self) -> &str {
        &self.name
    }

    fn kind(&self) -> EngineKind {
        EngineKind::File
    }

    fn open(&self) -> Result<EngineHandle, DbError> {
        let store = FileStore::new(&self.name, self.root.clone());
        store.connect()?;
        Ok(EngineHandle::Direct(Rc::new(store)))
    }
}
