//! The unified operation API's transport layer.
//!
//! [`MemoryBindingStore`] keeps per-transport bindings; [`TransportRegistry`] installs transports and computes an
//! [`Exposure`]: which operations each transport exposes, with which binding. [`StageMiddleware`] runs each transport's
//! hooks between the permission check and the handler. Contracts live in `speedyspec-transport-contracts`.
//!
//! Patterns: Registry (transports), Strategy (each transport's default mapping), Chain of Responsibility (stage
//! middleware).

mod bindings;
mod exposure;
mod registry;
mod stage;

pub use bindings::MemoryBindingStore;
pub use exposure::Exposure;
pub use registry::TransportRegistry;
pub use stage::StageMiddleware;
