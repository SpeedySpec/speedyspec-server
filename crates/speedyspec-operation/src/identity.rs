//! An in-memory `Identity`, for tests and simple setups.

use std::collections::HashSet;

use speedyspec_operation_contracts::{Identity, Value};

/// Grants capabilities to users. Ignores the capability's arguments.
#[derive(Default)]
pub struct MemoryIdentity {
    grants: HashSet<(u64, String)>,
}

impl MemoryIdentity {
    pub fn new() -> Self {
        MemoryIdentity::default()
    }

    pub fn grant(mut self, user: u64, capability: &str) -> Self {
        self.grants.insert((user, capability.to_owned()));
        self
    }
}

impl Identity for MemoryIdentity {
    fn can(&self, user: Option<u64>, capability: &str, _: &[Value]) -> bool {
        user.is_some_and(|u| self.grants.contains(&(u, capability.to_owned())))
    }
}
