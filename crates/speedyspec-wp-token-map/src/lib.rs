//! WordPress's `WP_Token_Map` in Rust: a map of short tokens to replacements, looked up as WordPress looks them up
//! (longest token first, case-sensitively or ASCII-case-insensitively), and the HTML5 named character references
//! as a table built at compile time.

mod map;

pub use map::{
    CaseSensitivity, MAX_LENGTH, STORAGE_VERSION, TokenMap, TokenMapView, TooLong, Words,
};
