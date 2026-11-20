//! The default unit of work: changes grouped by engine, one transaction per engine, callbacks after all commit.

use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_data::{
    ChangeKind, CommitError, CommitReport, DbError, EngineHandle, EngineLookup, PendingChange,
    UnitOfWork,
};

use crate::IdentityMap;

/// Collects changes for one request and commits them engine by engine (no two-phase commit).
pub struct DefaultUnitOfWork {
    lookup: Rc<dyn EngineLookup>,
    changes: RefCell<Vec<Box<dyn PendingChange>>>,
    callbacks: RefCell<Vec<Box<dyn FnOnce()>>>,
    identity_map: IdentityMap,
}

impl DefaultUnitOfWork {
    pub fn new(lookup: Rc<dyn EngineLookup>) -> DefaultUnitOfWork {
        DefaultUnitOfWork {
            lookup,
            changes: RefCell::new(Vec::new()),
            callbacks: RefCell::new(Vec::new()),
            identity_map: IdentityMap::new(),
        }
    }

    pub fn identity_map(&self) -> &IdentityMap {
        &self.identity_map
    }

    /// Engine names in commit order: the lookup's order, then others in first-registration order.
    fn engines(&self, changes: &[Box<dyn PendingChange>]) -> Vec<String> {
        let mut used: Vec<String> = Vec::new();
        for change in changes {
            if !used.iter().any(|e| e == change.engine()) {
                used.push(change.engine().to_string());
            }
        }
        let mut order: Vec<String> = self
            .lookup
            .commit_order()
            .into_iter()
            .filter(|e| used.contains(e))
            .collect();
        let rest: Vec<String> = used.into_iter().filter(|e| !order.contains(e)).collect();
        order.extend(rest);
        order
    }

    fn commit_engine(handle: &EngineHandle, changes: &[&dyn PendingChange]) -> Result<(), DbError> {
        let sql = handle.as_sql().filter(|s| s.capabilities().transactions);
        if let Some(store) = sql {
            store.begin()?;
            let applied = changes
                .iter()
                .try_for_each(|c| c.apply(handle))
                .and_then(|()| store.commit());
            if let Err(error) = applied {
                let _ = store.rollback();
                return Err(error);
            }
            Ok(())
        } else {
            changes.iter().try_for_each(|c| c.apply(handle))
        }
    }

    fn clear(&self) -> Vec<Box<dyn FnOnce()>> {
        self.changes.borrow_mut().clear();
        self.identity_map.clear();
        std::mem::take(&mut *self.callbacks.borrow_mut())
    }
}

impl UnitOfWork for DefaultUnitOfWork {
    fn register(&self, change: Box<dyn PendingChange>) {
        self.changes.borrow_mut().push(change);
    }

    fn after_commit(&self, callback: Box<dyn FnOnce()>) {
        self.callbacks.borrow_mut().push(callback);
    }

    fn commit(&self) -> Result<CommitReport, CommitError> {
        let changes = std::mem::take(&mut *self.changes.borrow_mut());
        let result = self.commit_changes(&changes);
        let callbacks = self.clear();
        if result.is_ok() {
            for callback in callbacks {
                callback();
            }
        }
        result
    }

    fn discard(&self) {
        self.clear();
    }

    fn is_empty(&self) -> bool {
        self.changes.borrow().is_empty()
    }
}

impl DefaultUnitOfWork {
    fn commit_changes(
        &self,
        changes: &[Box<dyn PendingChange>],
    ) -> Result<CommitReport, CommitError> {
        let engines = self.engines(changes);
        let mut handles = Vec::with_capacity(engines.len());
        for engine in &engines {
            let handle = self
                .lookup
                .handle(engine)
                .map_err(|error| CommitError::Failed {
                    engine: engine.clone(),
                    error,
                })?;
            if let Some(store) = handle.as_sql()
                && store.in_transaction()
            {
                return Err(CommitError::Failed {
                    engine: engine.clone(),
                    error: DbError::Transaction(format!(
                        "a transaction opened outside the unit of work is open on {engine}"
                    )),
                });
            }
            handles.push(handle);
        }
        let mut committed = Vec::new();
        for (engine, handle) in engines.iter().zip(&handles) {
            let mut ordered: Vec<&dyn PendingChange> = Vec::new();
            for kind in [ChangeKind::New, ChangeKind::Dirty, ChangeKind::Removed] {
                ordered.extend(
                    changes
                        .iter()
                        .filter(|c| c.engine() == engine && c.kind() == kind)
                        .map(|c| c.as_ref()),
                );
            }
            if let Err(error) = Self::commit_engine(handle, &ordered) {
                return Err(if committed.is_empty() {
                    CommitError::Failed {
                        engine: engine.clone(),
                        error,
                    }
                } else {
                    CommitError::Partial {
                        committed,
                        failed: engine.clone(),
                        error,
                    }
                });
            }
            committed.push(engine.clone());
        }
        Ok(CommitReport { committed })
    }
}
