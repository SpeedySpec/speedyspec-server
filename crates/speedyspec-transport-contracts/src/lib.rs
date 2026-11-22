//! The unified operation API's transport contracts.
//!
//! Ports a transport (REST, XML-RPC, Abilities, later WebSocket and GraphQL) implements or uses, and the value objects
//! between them. Depends only on `speedyspec-operation-contracts`. Implementations live in `speedyspec-transport` and
//! in each transport's own crate.
//!
//! Patterns: Ports and Adapters (every trait here is a port; transports are driving adapters); Strategy
//! ([`Transport::default_binding`], each transport's default mapping); Template Method (the serve pipeline in
//! `speedyspec-transport` runs [`Authenticator`] and [`Codec`] in a fixed order).

mod exposure;
mod name;
mod ports;

pub use exposure::{BindingSource, ExposedOperation, ExposureError, ExposureLookup};
pub use name::{TransportName, TransportNameError};
pub use ports::{
    Authenticator, BindingStore, Codec, DecodedCall, StageHooks, Transport, TransportHooks,
};
