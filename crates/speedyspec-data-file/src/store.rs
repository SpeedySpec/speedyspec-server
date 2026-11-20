//! `FileStore`: the file Direct Store; `as_any()` is its `FileClient`.

use std::any::Any;
use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use speedyspec_data::{DbError, DirectStore, Health};

use crate::FileClient;

pub struct FileStore {
    engine: String,
    connected: Rc<Cell<bool>>,
    client: FileClient,
}

impl FileStore {
    pub fn new(engine: &str, root: PathBuf) -> FileStore {
        let connected = Rc::new(Cell::new(false));
        FileStore {
            engine: engine.to_string(),
            client: FileClient::new(engine, root, connected.clone()),
            connected,
        }
    }

    pub fn client(&self) -> &FileClient {
        &self.client
    }
}

impl DirectStore for FileStore {
    fn engine(&self) -> &str {
        &self.engine
    }

    fn kind(&self) -> &'static str {
        "file"
    }

    fn connect(&self) -> Result<(), DbError> {
        std::fs::create_dir_all(self.client.root()).map_err(|e| DbError::Connection {
            engine: self.engine.clone(),
            message: format!("{}: {e}", self.client.root().display()),
        })?;
        self.connected.set(true);
        Ok(())
    }

    fn health(&self) -> Result<Health, DbError> {
        if !self.connected.get() {
            return Ok(Health {
                ok: false,
                detail: "closed".into(),
            });
        }
        let probe = self
            .client
            .root()
            .join(format!(".speedyspec-health-{}", std::process::id()));
        let result = std::fs::write(&probe, b"ok").and_then(|()| std::fs::remove_file(&probe));
        Ok(match result {
            Ok(()) => Health {
                ok: true,
                detail: format!("{} is writable", self.client.root().display()),
            },
            Err(e) => Health {
                ok: false,
                detail: format!("{}: {e}", self.client.root().display()),
            },
        })
    }

    fn close(&self) -> Result<(), DbError> {
        self.connected.set(false);
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        &self.client
    }
}
