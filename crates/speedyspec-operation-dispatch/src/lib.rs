//! Dispatch for the unified operation API.
//!
//! Handlers are bound in the shared [`speedyspec_container::Container`] under each operation's name
//! ([`bind_handler`]), so binding the same name again replaces a handler, from Rust or PHP. [`Dispatcher::call`] runs
//! one call: validate the neutral input, hydrate the input value object, run the middleware chain (permission first)
//! around the handler, extract the output value object and validate it.
//!
//! `speedyspec-cqrs`'s `Middleware` cannot be driven from another crate (its `Next` has no public constructor), so
//! this crate has its own [`OperationMiddleware`], the same Chain of Responsibility shape.
//!
//! Patterns: Command/Query dispatch by key, Chain of Responsibility (middleware), Data Mapper (hydrate/extract).

mod dispatcher;
mod handler;
mod middleware;

pub use dispatcher::Dispatcher;
pub use handler::{OperationHandler, bind_handler, handler_fn, handler_key};
pub use middleware::{Bypass, Call, OperationMiddleware, PermissionMiddleware};
