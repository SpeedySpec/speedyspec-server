//! WordPress's UTF-8 functions as native PHP functions, over `speedyspec-wp-utf8` (Facade, Adapter).
//!
//! [`Utf8Area`] declares, at the start of every request (from `utf8.php`), the functions of WordPress 7.1.5's
//! `utf8.php` and `compat-utf8.php` and compat.php's `_is_utf8_charset`, `_mb_substr` and `_mb_strlen`, with
//! WordPress's signatures. Each is a thin PHP function, so by-reference arguments and PHP's argument coercion work
//! as in WordPress, over a native function under `SpeedySpec\WpUtf8\Native\` that does the work in Rust.

mod native;
mod release;

use speedyspec_php::NativeArea;
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;

pub use release::{PORTED_FUNCTIONS, prepare_release, strip_ported};

const FACADE: &str = include_str!("utf8.php");

/// The native area of WordPress's UTF-8 functions.
pub struct Utf8Area;

impl NativeArea for Utf8Area {
    fn functions(&self) -> Vec<FunctionBuilder<'static>> {
        native::functions()
    }

    fn request_start(&self) {
        if let Err(err) = speedyspec_php::eval("speedyspec://wp-utf8", FACADE) {
            eprintln!("speedyspec_wordpress: installing the UTF-8 functions failed: {err}");
        }
    }
}
