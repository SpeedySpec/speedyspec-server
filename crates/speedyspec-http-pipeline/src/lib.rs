//! The runtime's request pipeline.
//!
//! Every request becomes a server request and passes through one [`Pipeline`] (Front Controller): middleware in
//! order, each answering or passing on (Chain of Responsibility, PSR-15's shape), then a final handler, which in
//! the runtime runs PHP. [`StageMiddleware`] lets a `speedyspec-native-stage` stage, such as the firewall, sit in
//! the pipeline unchanged (Adapter).

mod pipeline;
mod stage;

pub use pipeline::Pipeline;
pub use stage::StageMiddleware;
