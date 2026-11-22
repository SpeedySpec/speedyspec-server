//! The password area's container bindings don't keep the Hook API alive, so they add no reference cycle through the
//! container (Container → binding → filters → Hook API → hook store → Container).
//!
//! The Hook API's own handlers, which `WpHooksArea::new` binds in the container, are outside this crate.

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_wp_crypto_sodium::Sodium;
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::WpHooksArea;
use speedyspec_wp_password_contracts::{PASSWORD_HASHER, PasswordHasher};
use speedyspec_wp_password_php::PasswordArea;

#[test]
fn bound_defaults_hold_no_strong_reference_to_the_hook_api() {
    let container = Rc::new(Container::new());
    let hooks = WpHooksArea::new(container.clone(), Rc::new(MemoryHookStore::new()));
    let api = hooks.api();
    let sodium = Rc::new(Sodium::init().expect("libsodium initializes"));
    let password = PasswordArea::new(api.clone(), container.clone(), sodium);
    let before = Rc::strong_count(&api);
    password.bind_defaults();
    let hasher = container
        .make_port::<dyn PasswordHasher>(PASSWORD_HASHER)
        .expect("the default hasher resolves");
    assert_eq!(Rc::strong_count(&api), before);
    drop(hasher);
}
