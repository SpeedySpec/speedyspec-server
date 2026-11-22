//! Operation descriptors.

use std::collections::BTreeSet;
use std::rc::Rc;

use crate::{Category, ContractMapper, PermissionCheck, Schema};

/// A stable operation name: `namespace/name`, lowercase letters, digits and dashes (the Abilities API's rule).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OperationName(String);

/// Why a string is not an operation name.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[error("{name} is not an operation name (namespace/name, lowercase letters, digits and dashes)")]
pub struct NameError {
    pub name: String,
}

impl OperationName {
    pub fn parse(name: &str) -> Result<OperationName, NameError> {
        let part = |s: &str| {
            !s.is_empty()
                && s.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        };
        match name.split_once('/') {
            Some((namespace, rest)) if part(namespace) && part(rest) => {
                Ok(OperationName(name.to_owned()))
            }
            _ => Err(NameError {
                name: name.to_owned(),
            }),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Query,
    Command,
    Subscription,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    List,
    Get,
    Create,
    Update,
    Delete,
    Custom(String),
}

impl Action {
    pub fn as_str(&self) -> &str {
        match self {
            Action::List => "list",
            Action::Get => "get",
            Action::Create => "create",
            Action::Update => "update",
            Action::Delete => "delete",
            Action::Custom(name) => name,
        }
    }
}

/// Which resource an operation acts on, how, and which input fields identify it.
#[derive(Clone, Debug, PartialEq)]
pub struct Resource {
    pub type_name: String,
    pub action: Action,
    pub identity: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pagination {
    Cursor,
    Offset,
}

/// An error an operation declares it may return.
#[derive(Clone, Debug, PartialEq)]
pub struct ErrorSpec {
    pub code: String,
    pub category: Category,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Labels {
    pub title: String,
    pub description: String,
}

/// One side of an operation's contract: its schema and the mapper to and from its value object.
#[derive(Clone)]
pub struct ContractSpec {
    pub schema: Schema,
    pub mapper: Rc<dyn ContractMapper>,
}

/// An operation's full contract. Protocol-neutral: the only transport names in it are the optional restriction.
#[derive(Clone)]
pub struct OperationSpec {
    pub name: OperationName,
    pub kind: Kind,
    pub resource: Option<Resource>,
    pub idempotent: bool,
    pub destructive: bool,
    pub pagination: Option<Pagination>,
    pub input: ContractSpec,
    pub output: ContractSpec,
    pub errors: Vec<ErrorSpec>,
    pub labels: Labels,
    pub deprecated: Option<String>,
    /// `None`: every transport.
    pub transports: Option<BTreeSet<String>>,
    pub permission: Rc<dyn PermissionCheck>,
}

impl OperationSpec {
    /// Whether `transport` may expose this operation.
    pub fn exposed_on(&self, transport: &str) -> bool {
        transport == crate::INTERNAL_TRANSPORT
            || self
                .transports
                .as_ref()
                .is_none_or(|t| t.contains(transport))
    }
}
