//! The option repository over `wp_options`, through the `wpdb` port: every statement goes through
//! `WpDb::query_statement`, so the `query` filter fires and `$wpdb->num_queries` counts it, as WordPress's own calls
//! do.

mod mapper;
mod repository;

pub use mapper::OptionMapper;
pub use repository::SqlOptionRepository;
