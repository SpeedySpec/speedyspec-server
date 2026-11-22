//! The Hook API area doesn't keep its container alive: once the area and every other owner are gone, the container
//! is freed, though the Hook API's handlers are bound in it and its hook store resolves through it.

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::WpHooksArea;

#[test]
fn dropping_the_area_frees_the_container() {
    let container = Rc::new(Container::new());
    let weak = Rc::downgrade(&container);
    let hooks = WpHooksArea::new(container.clone(), Rc::new(MemoryHookStore::new()));
    drop(hooks);
    drop(container);
    assert!(weak.upgrade().is_none());
}
