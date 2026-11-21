//! `FileCache`: a Caching Store (Strategy) in files, through the file Direct Store's `FileClient`.

mod cache;
mod layout;

pub use cache::FileCache;
pub use layout::{entry_path, group_dir, lock_path};
