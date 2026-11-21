//! WordPress's routing, as part of the router's core: query templates, PHP's `parse_str`, rule sets and permastruct
//! expansion.

mod parse_str;
mod permastruct;
mod php;
mod php_array;
mod query_template;
mod redirect;
mod rule_set;

pub use parse_str::php_parse_str;
pub use permastruct::{ExpandContext, generate_rewrite_rule, generate_rewrite_rules, preg_index};
pub use php::{php_addslashes, php_urldecode, php_urldecode_bytes, php_urlencode};
pub use php_array::PhpArray;
pub use query_template::{QueryTemplate, matches_map, matches_map_with};
pub use redirect::wp_sanitize_redirect;
pub use rule_set::RuleSetMatcher;
