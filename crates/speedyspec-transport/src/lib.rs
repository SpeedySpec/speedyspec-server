//! The unified operation API's transport layer.
//!
//! [`MemoryBindingStore`] keeps per-transport bindings; [`TransportRegistry`] installs transports and computes an
//! [`Exposure`]: which operations each transport exposes, with which binding. [`StageMiddleware`] runs each transport's
//! hooks between the permission check and the handler; [`Pipeline`] is the serve order every transport runs. Contracts live in `speedyspec-transport-contracts`.
//!
//! Patterns: Registry (transports), Strategy (each transport's default mapping), Chain of Responsibility (stage
//! middleware), Template Method (the pipeline).

mod bindings;
mod exposure;
pub mod naming;
mod pipeline;
mod registry;
mod stage;

pub use bindings::MemoryBindingStore;
pub use exposure::Exposure;
pub use pipeline::Pipeline;
pub use registry::TransportRegistry;
pub use stage::StageMiddleware;
