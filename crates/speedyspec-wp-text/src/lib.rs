//! WordPress's text formatting functions, in Rust.
//!
//! The content filters (`wptexturize`, `wpautop` and the rest) as pure functions over bytes, matching stock
//! WordPress byte for byte, and a `TextApi` facade that fires WordPress's filters around them.

pub mod preg;
pub mod strings;
