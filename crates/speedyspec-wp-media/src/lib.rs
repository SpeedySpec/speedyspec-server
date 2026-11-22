//! WordPress's media metadata rules over the media reader port.
//!
//! This crate depends only on `speedyspec-wp-media-contracts`. It holds the composite reader that chooses a reader
//! per file ([`SniffingReader`]); WordPress's rules (the getID3 view, the audio, video and image functions, MIME
//! tables) join it in later plans.

mod sniffing;

pub use sniffing::SniffingReader;
