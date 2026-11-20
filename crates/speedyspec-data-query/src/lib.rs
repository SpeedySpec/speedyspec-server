//! Parsing, building and routing statements for speedyspec's data layer.
//!
//! Every SQL statement is parsed (Interpreter) with MySQL's dialect; there is no pass-through path.

mod parse;

pub use parse::{parse_many, parse_one};
