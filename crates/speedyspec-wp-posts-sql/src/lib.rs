//! `wp_posts` over the `wpdb` port: the post repository and the row mapper.

mod mapper;
mod repository;

pub use mapper::PostMapper;
pub use repository::SqlPostRepository;
