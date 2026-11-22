//! The ports: what the theme.json engine needs from outside, each implemented by an adapter elsewhere.

mod block_metadata;
mod diagnostics;
mod gettext;
mod text;

pub use block_metadata::{BlockMetadata, RegisteredBlock};
pub use diagnostics::Diagnostics;
pub use gettext::Gettext;
pub use text::TextFunctions;
