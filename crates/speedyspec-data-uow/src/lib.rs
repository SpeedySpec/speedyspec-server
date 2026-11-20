//! The unit of work (Unit of Work) and identity map (Identity Map) for speedyspec's data layer.
//!
//! Changes are grouped by engine and committed in one transaction per engine; there is no two-phase commit, so a
//! failure after some engines committed is `CommitError::Partial`. Callbacks run only when every engine committed.

mod identity_map;
mod unit_of_work;

pub use identity_map::IdentityMap;
pub use unit_of_work::DefaultUnitOfWork;
