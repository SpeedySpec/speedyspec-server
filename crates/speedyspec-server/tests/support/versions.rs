//! Server versions for the tests to upgrade to. They are always above this binary's own version: a literal such as
//! "0.2.0" stops working the day a release bumps the binary to it (a switch to the running version does nothing, and
//! the test hooks keyed on the target version then fire on the binary under test).

use std::sync::LazyLock;

fn own() -> (u64, u64) {
    let mut parts = env!("CARGO_PKG_VERSION")
        .split('.')
        .map(|part| part.parse::<u64>().unwrap());
    (parts.next().unwrap(), parts.next().unwrap())
}

/// One minor above this binary's own version.
pub fn next() -> &'static str {
    static NEXT: LazyLock<String> = LazyLock::new(|| {
        let (major, minor) = own();
        format!("{major}.{}.0", minor + 1)
    });
    &NEXT
}

/// Two minors above this binary's own version, newer than [`next`].
pub fn after_next() -> &'static str {
    static AFTER_NEXT: LazyLock<String> = LazyLock::new(|| {
        let (major, minor) = own();
        format!("{major}.{}.0", minor + 2)
    });
    &AFTER_NEXT
}
