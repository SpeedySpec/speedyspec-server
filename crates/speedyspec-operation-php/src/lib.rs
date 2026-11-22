//! The unified operation API from PHP.
//!
//! [`OperationArea`] declares, at every request start, the PHP facade `SpeedySpec\Operation\{S, Schema, Type,
//! Operation, Operations, Category, RegistrationException}` and the functions `wp_register_operation` and
//! `wp_operation_transports`, over native functions that register into the site's Rust registries.
//!
//! - [`convert`] turns neutral values into the PHP values that cross the native boundary, and back.
//! - [`shape`] checks a value object class (as PHP reflection describes it) against its schema at registration.
//! - [`descriptor`] reads the facade's descriptor arrays into specifications.
//! - [`mapper`] holds the Data Mappers between neutral values and PHP value objects.
//! - [`permission`] runs PHP permission callbacks.
//! - [`sites`] keeps one set of registries per site, per worker.
//!
//! PHP callables (handlers, permission callbacks and the closures they hold) live for one request, like every PHP
//! value Rust holds. Specifications live for the worker. So the area fires `wp_operations_init` once per request, the
//! first time the request needs operations: the first run fills and closes the site's registries; later runs
//! re-register identical definitions (no-ops) and so re-bind that request's callables.

mod area;
pub mod convert;
pub mod descriptor;
pub mod handler;
pub mod identity;
pub mod invoke;
pub mod mapper;
pub mod permission;
pub mod shape;
pub mod sites;

pub use area::{INIT_HOOK, OperationArea, php_caller};
pub use identity::PhpIdentity;
pub use sites::{FixedSiteKey, PhpSiteKey, Site, SiteId, SiteKey, Sites};
