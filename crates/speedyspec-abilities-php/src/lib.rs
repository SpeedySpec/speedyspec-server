//! The Abilities transport's PHP adapter.
//!
//! - [`PhpAbilityHooks`] fires WordPress's ability hooks (`wp_ability_invoked`, `wp_pre_execute_ability` with its
//!   sentinel, the step filters, `wp_before_execute_ability`, `wp_after_execute_ability`) with WordPress's arguments
//!   and coercions, failing closed when one throws.
//! - [`errors`] maps `WP_Error` to and from operation errors exactly; [`handles`] keeps the request's `WP_Ability`
//!   objects for the hooks.
//! - [`AbilitiesArea`] gives PHP `SpeedySpec\Abilities\Abilities::publish( $operation )->category( … )->…`.

mod area;
pub mod errors;
pub mod handles;
mod hooks;

pub use area::AbilitiesArea;
pub use hooks::PhpAbilityHooks;
