//! WordPress's own cache tests against the native object cache on a multisite install (`is_multisite()` true).

mod common;

fn main() {
    // Before PHP boots, so the bootstrap's `is_multisite()` reads it.
    // SAFETY: nothing else runs yet; this process has one thread.
    unsafe { std::env::set_var("SPEEDYSPEC_TEST_MULTISITE", "1") };
    common::run("wordpress-object-cache-multisite");
}
