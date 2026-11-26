//! The option repository over `wp_options`, through the `Wpdb` port: every statement goes through
//! `Wpdb::query_statement`, so the `query` filter fires and `$wpdb->num_queries` counts it, as WordPress's own calls
//! do.

mod expired;
mod mapper;
mod repository;
mod sitemeta;

pub use expired::SqlExpiredTransients;
pub use mapper::OptionMapper;
pub use repository::SqlOptionRepository;
pub use sitemeta::{SitemetaMapper, SqlSitemetaRepository};
