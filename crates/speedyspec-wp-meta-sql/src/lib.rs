//! The meta tables over the `wpdb` port: the repository per meta type, its factory, and the row mapper.

mod mapper;
mod repository;

pub use mapper::MetaMapper;
pub use repository::{SqlMetaRepositories, SqlMetaRepository};
