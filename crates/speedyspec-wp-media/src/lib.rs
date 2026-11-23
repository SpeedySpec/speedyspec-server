//! WordPress's media metadata rules over the media reader port.
//!
//! This crate depends only on `speedyspec-wp-media-contracts`. It holds the composite reader that chooses a reader
//! per file ([`SniffingReader`]) and WordPress's rules: the getID3 view, the audio, video and image functions, and
//! the MIME tables.

pub mod functions;
pub mod getid3;
pub mod images;
pub mod mime;
pub mod neutral;
pub mod php;
mod sniffing;

pub use sniffing::SniffingReader;
