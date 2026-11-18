//! A state store in memory.

use std::path::PathBuf;
use std::sync::Mutex;

use speedyspec_server_domain::{ReleaseState, StateError, StateStore};

/// Keeps the state in memory; can be told to fail the next save.
#[derive(Default)]
pub struct MemoryStateStore {
    inner: Mutex<(ReleaseState, bool, usize)>,
}

impl MemoryStateStore {
    pub fn new() -> MemoryStateStore {
        MemoryStateStore::default()
    }

    /// Starts from `state` instead of the default.
    pub fn with(state: ReleaseState) -> MemoryStateStore {
        MemoryStateStore {
            inner: Mutex::new((state, false, 0)),
        }
    }

    pub fn fail_next_save(&self) {
        self.inner.lock().unwrap().1 = true;
    }

    /// Successful saves so far.
    pub fn saves(&self) -> usize {
        self.inner.lock().unwrap().2
    }
}

impl StateStore for MemoryStateStore {
    fn load(&self) -> Result<ReleaseState, StateError> {
        Ok(self.inner.lock().unwrap().0.clone())
    }

    fn save(&self, state: &ReleaseState) -> Result<(), StateError> {
        let mut inner = self.inner.lock().unwrap();
        if inner.1 {
            inner.1 = false;
            return Err(StateError {
                path: PathBuf::from("memory"),
                message: "disk full".to_string(),
            });
        }
        inner.0 = state.clone();
        inner.2 += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_and_fails_on_request() {
        let store = MemoryStateStore::new();
        let mut state = ReleaseState::default();
        state.revision = 2;
        store.save(&state).unwrap();
        assert_eq!(store.load().unwrap().revision, 2);
        store.fail_next_save();
        assert!(store.save(&state).is_err());
        assert!(store.save(&state).is_ok());
        assert_eq!(store.saves(), 2);
    }
}
