//! The password area's container bindings, without PHP running: they add no reference cycle through the container,
//! and the site's cost limits are whatever is bound under their port.

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_wp_crypto_sodium::Sodium;
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::WpHooksArea;
use speedyspec_wp_password::FixedCostLimits;
use speedyspec_wp_password_contracts::{
    CostLimits, PASSWORD_COST_LIMITS, PASSWORD_HASHER, PasswordCostLimits, PasswordError,
    PasswordHasher,
};
use speedyspec_wp_password_php::PasswordArea;

fn area() -> (Rc<Container>, WpHooksArea, PasswordArea) {
    let container = Rc::new(Container::new());
    let hooks = WpHooksArea::new(container.clone(), Rc::new(MemoryHookStore::new()));
    let sodium = Rc::new(Sodium::init().expect("libsodium initializes"));
    let password = PasswordArea::new(hooks.api(), container.clone(), sodium);
    password.bind_defaults();
    (container, hooks, password)
}

/// The Hook API's own handlers, which `WpHooksArea::new` binds in the container, are outside this crate: only the
/// password area's share is checked.
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

#[test]
fn default_cost_limits_are_bound() {
    let (container, _hooks, _password) = area();
    let limits = container
        .make_port::<dyn PasswordCostLimits>(PASSWORD_COST_LIMITS)
        .map(|port| port.limits());
    assert_eq!(limits.ok(), Some(Ok(CostLimits::DEFAULT)));
}

#[test]
fn a_site_binding_its_cost_limits_caps_the_default_hasher() {
    let (container, _hooks, _password) = area();
    container.bind_port::<dyn PasswordCostLimits>(PASSWORD_COST_LIMITS, |_, _| {
        Ok(Rc::new(FixedCostLimits(CostLimits {
            argon2_memory_kib: 1024,
            ..CostLimits::DEFAULT
        })) as Rc<dyn PasswordCostLimits>)
    });
    let hashed = container
        .make_port::<dyn PasswordHasher>(PASSWORD_HASHER)
        .map(|hasher| hasher.hash(b"pw"));
    assert_eq!(
        hashed.ok(),
        Some(Err(PasswordError::value_error(
            "Memory cost is outside of allowed memory range"
        )))
    );
}
