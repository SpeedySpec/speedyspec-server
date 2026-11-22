//! The unified operation API's transport layer.
//!
//! [`MemoryBindingStore`] keeps per-transport bindings; [`TransportRegistry`] installs transports and computes an
//! [`Exposure`]: which operations each transport exposes, with which binding. Later modules add the stage middleware
//! for hooks and the serve pipeline. Contracts live in `speedyspec-transport-contracts`.
//!
//! Patterns: Registry (transports), Strategy (each transport's default mapping), Adapter (bindings store).

mod bindings;
mod exposure;
mod registry;

pub use bindings::MemoryBindingStore;
pub use exposure::Exposure;
pub use registry::TransportRegistry;
