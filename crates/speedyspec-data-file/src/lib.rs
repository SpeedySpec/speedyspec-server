//! The file Direct Store (Adapter over a directory): files written atomically, read by relative path, locked with
//! `flock`; repositories reach the `FileClient` through `DirectStore::as_any`.

mod client;
mod factory;
mod store;

pub use client::{FileClient, FileLock};
pub use factory::FileEngineFactory;
pub use store::FileStore;
