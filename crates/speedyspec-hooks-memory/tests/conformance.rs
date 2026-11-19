use std::rc::Rc;

use speedyspec_hooks::SharedHooks;
use speedyspec_hooks::conformance::{Capabilities, run_all};
use speedyspec_hooks_memory::MemoryHooks;

#[test]
fn memory_backend_conforms() {
    run_all(
        &|| Rc::new(MemoryHooks::new()) as SharedHooks,
        Capabilities {
            carries_opaque: true,
        },
    );
}
