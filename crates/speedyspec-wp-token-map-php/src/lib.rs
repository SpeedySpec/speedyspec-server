//! WordPress's `WP_Token_Map` in PHP over `speedyspec-wp-token-map` (Facade).
//!
//! [`TokenMapArea`] is the native area for the token map: WordPress's class, with the HTML5 named character
//! reference table WordPress builds (`$html5_named_character_references`) answered natively.

mod release;

use speedyspec_php::NativeArea;
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;

pub use release::prepare_release;

/// The native area of WordPress's token map.
pub struct TokenMapArea;

impl NativeArea for TokenMapArea {
    fn functions(&self) -> Vec<FunctionBuilder<'static>> {
        Vec::new()
    }
}
