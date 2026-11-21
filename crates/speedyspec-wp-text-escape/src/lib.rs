//! WordPress's escaping functions, in Rust.
//!
//! Every escaping function in `formatting.php` gets its exact bytes from PHP's `htmlspecialchars()`, so this crate
//! ports that too ([`htmlspecialchars`]), with PHP's charset table ([`Charset`]) and HTML 4.01 entity names. The
//! cores take bytes and return `Cow`, borrowing when nothing needs escaping.

mod charset;
mod decode;
mod entities2;
mod html401;
mod htmlspecialchars;
mod js;
mod legacy;
mod slash;
mod tag;

pub use charset::{Charset, SingleByte, canonical_charset};
pub use decode::{QuoteStyle, wp_specialchars_decode};
pub use entities2::htmlentities2;
pub use htmlspecialchars::{EntFlags, htmlspecialchars};
pub use js::esc_js_after_specialchars;
pub use legacy::seems_utf8_core;
pub use slash::{addslashes, backslashit, stripslashes, wp_slash_value, wp_unslash_value};
pub use tag::tag_escape_core;
