//! Default adapters for the text ports, for Rust callers and tests without PHP.

mod escape;
mod link;
mod mime;
pub(crate) mod mime_table;
mod random;
mod smilies;

pub use escape::{EscapeAttributes, FixedUploads, NoUnfilteredHtml, kses_for};
pub use link::HostLinkContext;
pub use mime::FixedMimeTypes;
pub use random::PhpMtRand;
pub use smilies::{FixedSmilies, NoSmilies};
