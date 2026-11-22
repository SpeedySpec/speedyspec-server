//! Presets: the values a theme offers (colours, sizes, …) and what is computed from them.

pub mod fluid;
pub mod kebab;
pub mod spacing;
pub mod vars;

pub use fluid::{
    FluidFontSize, ValueAndUnit, computed_fluid_typography_value, typography_value_and_unit,
};
pub use kebab::to_kebab_case;
