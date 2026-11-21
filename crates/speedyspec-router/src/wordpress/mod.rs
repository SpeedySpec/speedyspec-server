//! WordPress's routing, as part of the router's core: query templates, PHP's `parse_str`, rule sets and permastruct
//! expansion.

mod parse_str;
mod php;
mod query_template;

pub use parse_str::php_parse_str;
pub use php::{php_addslashes, php_urldecode, php_urlencode};
pub use query_template::QueryTemplate;
