//! The PHP twin interface of the `wp_text_diff()` port, which PHP implements to replace it.
//!
//! `#[php_port]` declares the interface at the start of every request. Values cross untyped: WordPress's function
//! takes and returns any PHP value. Calls into a PHP implementation go through [`crate::adapters`], which keeps the
//! exception a replacement throws so the caller sees it unchanged.

use speedyspec_php_ext::{PortError, Value, php_port};

/// `wp_text_diff()`'s twin.
#[php_port(name = "SpeedySpec\\WpTextDiff\\TextDiffer")]
pub trait TextDifferTwin {
    fn diff(
        &self,
        left_string: Value,
        right_string: Value,
        args: Value,
    ) -> Result<Value, PortError>;
}
