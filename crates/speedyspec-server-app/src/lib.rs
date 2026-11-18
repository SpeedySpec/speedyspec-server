//! Use cases of the SpeedySpec server. Depends only on the domain crate.

pub mod compat;

pub use compat::{Incompatibility, check_set};
