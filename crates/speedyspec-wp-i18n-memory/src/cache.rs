//! Translation files kept for the life of a worker process (Identity Map), one per canonical path.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use speedyspec_wp_i18n_contracts::{FileIdentity, TranslationFileCache};

/// One file per canonical path: a hit needs the whole identity (device, inode, size, modification time) to match,
/// and a new version replaces the old, so the cache never holds more than the files the worker has loaded. Each
/// worker builds its own from its own disk; nothing is shared between workers or replicas.
pub struct MemoryTranslationFileCache<F> {
    by_path: RefCell<HashMap<PathBuf, (FileIdentity, Rc<F>)>>,
}

impl<F> Default for MemoryTranslationFileCache<F> {
    fn default() -> Self {
        MemoryTranslationFileCache {
            by_path: RefCell::new(HashMap::new()),
        }
    }
}

impl<F> MemoryTranslationFileCache<F> {
    /// How many files are kept.
    pub fn len(&self) -> usize {
        self.by_path.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<F> TranslationFileCache<F> for MemoryTranslationFileCache<F> {
    fn get(&self, identity: &FileIdentity) -> Option<Rc<F>> {
        self.by_path
            .borrow()
            .get(&identity.path)
            .filter(|(cached, _)| cached == identity)
            .map(|(_, file)| file.clone())
    }

    fn put(&self, identity: FileIdentity, file: Rc<F>) {
        self.by_path
            .borrow_mut()
            .insert(identity.path.clone(), (identity, file));
    }
}
