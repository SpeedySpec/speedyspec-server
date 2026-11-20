//! The request state, resolved through the container so a PHP state bound under `WPDB_STATE` replaces the memory one.

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_wp_db::{MemoryWpdbState, RequestState, WPDB_STATE, WpdbState};

/// The `WpdbState` bound in the container now, else the worker's memory state.
pub struct ResolvedWpdbState {
    container: Rc<Container>,
    memory: Rc<MemoryWpdbState>,
}

impl ResolvedWpdbState {
    pub fn new(container: Rc<Container>, memory: Rc<MemoryWpdbState>) -> ResolvedWpdbState {
        ResolvedWpdbState { container, memory }
    }

    fn current(&self) -> Rc<dyn WpdbState> {
        self.container
            .make_port::<dyn WpdbState>(WPDB_STATE)
            .unwrap_or_else(|_| self.memory.clone())
    }
}

impl WpdbState for ResolvedWpdbState {
    fn read(&self, f: &mut dyn FnMut(&RequestState)) {
        self.current().read(f)
    }
    fn modify(&self, f: &mut dyn FnMut(&mut RequestState)) {
        self.current().modify(f)
    }
    fn reset(&self) {
        self.current().reset()
    }
}
