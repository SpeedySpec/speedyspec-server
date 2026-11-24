//! The Abilities transport.
//!
//! [`DefaultAbilityMapping`] gives an operation's ability metadata from its traits (Strategy), and
//! [`AbilitiesTransport`] installs it as the `abilities` transport. [`AbilitiesServer`] runs `WP_Ability::execute()`
//! in WordPress's hook order (Template Method) over an ability's [`AbilitySteps`](speedyspec_abilities_contracts::AbilitySteps)
//! (Strategy): [`OperationSteps`] for an operation. [`errors`] builds WordPress's ability errors. Contracts live in
//! `speedyspec-abilities-contracts`.

pub mod errors;
mod mapping;
mod server;
mod steps;
mod transport;

pub use mapping::{DefaultAbilityMapping, category_slug};
pub use server::AbilitiesServer;
pub use steps::OperationSteps;
pub use transport::{ABILITIES, AbilitiesTransport};
