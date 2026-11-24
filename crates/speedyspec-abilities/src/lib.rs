//! The Abilities transport.
//!
//! [`DefaultAbilityMapping`] gives an operation's ability metadata from its traits (Strategy), and
//! [`AbilitiesTransport`] installs it as the `abilities` transport. Contracts live in `speedyspec-abilities-contracts`.

mod mapping;
mod transport;

pub use mapping::{DefaultAbilityMapping, category_slug};
pub use transport::{ABILITIES, AbilitiesTransport};
