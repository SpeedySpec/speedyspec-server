//! `wp_using_ext_object_cache()` as a port: WordPress's tests toggle it in PHP, so PHP answers there, and in Rust
//! the object cache answers whether it has a persistent second level.

use std::rc::Rc;

use speedyspec_wp_object_cache::WpObjectCache;

/// Whether an external (persistent) object cache is in use.
pub trait ExternalObjectCache {
    fn using(&self) -> bool;
}

/// The Rust adapter: persistent exactly when the object cache has a second level.
pub struct ObjectCachePersistence(pub Rc<WpObjectCache>);

impl ExternalObjectCache for ObjectCachePersistence {
    fn using(&self) -> bool {
        self.0.is_persistent()
    }
}
