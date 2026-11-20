//! WordPress's own cache tests (tests/php/wordpress/cache.php, copied from wordpress-develop) pass against the
//! native object cache on a single site, run by the pinned PHPUnit inside the embedded PHP.

mod common;

fn main() {
    common::run("wordpress-object-cache");
}
