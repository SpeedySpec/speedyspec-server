//! The unified operation API from PHP.
//!
//! - [`convert`] turns neutral values into the PHP values that cross the native boundary, and back.
//! - [`shape`] checks a value object class (as PHP reflection describes it) against its schema at registration.
//!
//! PHP callables (handlers, permission callbacks and the closures they hold) live for one request, like every PHP
//! value Rust holds. Specifications live for the worker; each request re-binds its callables.

pub mod convert;
pub mod shape;
