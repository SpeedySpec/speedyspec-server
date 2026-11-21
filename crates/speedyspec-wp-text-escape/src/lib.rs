//! WordPress's escaping functions, in Rust.
//!
//! Every escaping function in `formatting.php` gets its exact bytes from PHP's `htmlspecialchars()`, so this crate
//! ports that too ([`htmlspecialchars`]), with PHP's charset table ([`Charset`]) and HTML 4.01 entity names. The
//! cores take bytes and return `Cow`, borrowing when nothing needs escaping.

mod charset;
// HTML401_TABLE is for htmlentities2, which arrives in a later task.
#[allow(dead_code)]
mod html401;
mod htmlspecialchars;

pub use charset::{Charset, SingleByte, canonical_charset};
pub use htmlspecialchars::{EntFlags, htmlspecialchars};
