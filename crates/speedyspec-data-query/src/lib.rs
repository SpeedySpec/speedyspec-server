//! Parsing, building and routing statements for speedyspec's data layer.
//!
//! Every SQL statement is parsed (Interpreter) with MySQL's dialect; there is no pass-through path.

mod parse;

pub use parse::{parse_many, parse_one};

mod builder;

pub use builder::{Order, Query};

mod partial;

pub use partial::{Clause, Partial};

mod passes;
mod registry;

pub use passes::{PassChain, PassContext, Rewrite, RewritePass, Rewritten};
pub use registry::EngineRegistry;

mod mysql_text;

pub use mysql_text::mysql_text;
