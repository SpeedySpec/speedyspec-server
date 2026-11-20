//! WordPress's object cache (`WP_Object_Cache`, `wp_cache_*`) in Rust.
//!
//! [`ObjectCache`] holds values as they are (the first level) and, when given a [`CachingStore`], keeps them in a
//! second level in PHP's serialize format (Decorator). Its ports ([`ObjectCachePorts`]) are what only the host knows:
//! the site, notices, cache suspension, and the codec.
//!
//! [`CachingStore`]: speedyspec_cache::CachingStore

mod cache;
mod codec;
mod error;
mod keys;
mod numeric;
mod persistent;
mod ports;

pub use cache::{CacheValue, ObjectCache};
pub use codec::SerializeCodec;
pub use error::ObjectCacheError;
pub use keys::array_key;
pub use ports::{
    AdditionSuspension, CacheNotices, CodecError, LoggedNotices, NeverSuspended, ObjectCachePorts,
    SingleSite, SiteContext, ValueCodec,
};

mod current;
mod messages;

pub use current::CurrentObjectCache;
pub use messages::*;
