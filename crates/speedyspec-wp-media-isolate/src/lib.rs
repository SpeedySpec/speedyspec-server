//! Reads untrusted media in a short-lived, resource-limited child (Protection Proxy), with a shared result cache in
//! front (Decorator).
//!
//! The child is a copy of the runtime binary (its hidden `media-probe` entry point) or, in tests, the
//! `speedyspec-media-probe` binary; both run [`child::run`]. It receives the file as stdin, reads it with the composite
//! reader (MIDI or FFmpeg), and writes the outcome in the [`wire`] format to stdout.

pub mod child;
mod isolated;
mod log;
#[cfg(target_os = "macos")]
mod rss;
mod sandbox;
pub mod wire;

pub use isolated::{IsolatedReader, ProbeCommand};
