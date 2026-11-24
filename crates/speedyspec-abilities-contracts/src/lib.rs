//! The Abilities transport's contracts.
//!
//! [`AbilityBinding`] holds the metadata an operation is published with as an ability (label, description, category,
//! visibility, [`Annotations`], meta). The ports are [`AbilityMapping`] (default metadata, Strategy), [`AbilityHooks`]
//! (WordPress's ability hooks) and [`AbilitySteps`] (how one ability normalizes, validates, authorizes, runs and
//! checks its output, Strategy). Depends only on `speedyspec-operation-contracts`. Implementations:
//! `speedyspec-abilities` and `speedyspec-abilities-php`.

mod binding;
mod ports;

pub use binding::{AbilityBinding, Annotations, is_ability_name, is_category_slug};
pub use ports::{
    AbilityHooks, AbilityMapping, AbilityOutcome, AbilityResult, AbilitySteps, NoAbilityHooks,
};
