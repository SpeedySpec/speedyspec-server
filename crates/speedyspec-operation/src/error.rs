//! Why a type or operation could not be registered.

/// A registration failure. Registration fails loudly so a broken contract never reaches a request.
#[derive(Clone, Debug, PartialEq)]
pub enum RegistrationError {
    InvalidName {
        name: String,
    },
    Duplicate {
        name: String,
    },
    DuplicateErrorCode {
        operation: String,
        code: String,
    },
    MissingContract {
        operation: String,
        which: &'static str,
    },
    MissingPermission {
        operation: String,
    },
    DestructiveQuery {
        operation: String,
    },
    InvalidSchema {
        at: String,
        why: String,
    },
    UnknownType {
        at: String,
        type_name: String,
    },
    UnknownOperation {
        at: String,
        operation: String,
    },
    RelationTargetNotQuery {
        type_name: String,
        field: String,
        operation: String,
    },
    RelationArgMissing {
        type_name: String,
        field: String,
        arg: String,
    },
    RelationSourceMissing {
        type_name: String,
        field: String,
        source: String,
    },
    RelationFieldCollides {
        type_name: String,
        field: String,
    },
    Closed {
        name: String,
    },
}

// Written by hand: thiserror would take the `source` field of `RelationSourceMissing` as the error's cause.
impl std::fmt::Display for RegistrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegistrationError::InvalidName { name } => write!(
                f,
                "{name} is not an operation name (namespace/name, lowercase letters, digits and dashes)"
            ),
            RegistrationError::Duplicate { name } => write!(f, "{name} is already registered"),
            RegistrationError::DuplicateErrorCode { operation, code } => {
                write!(f, "{operation} declares the error {code} twice")
            }
            RegistrationError::MissingContract { operation, which } => {
                write!(f, "{operation} has no {which} contract")
            }
            RegistrationError::MissingPermission { operation } => write!(
                f,
                "{operation} has no permission check (call public() to allow everyone)"
            ),
            RegistrationError::DestructiveQuery { operation } => {
                write!(f, "{operation} is a query and cannot be destructive")
            }
            RegistrationError::InvalidSchema { at, why } => {
                write!(f, "invalid schema at {at}: {why}")
            }
            RegistrationError::UnknownType { at, type_name } => {
                write!(f, "{at} refers to the unknown type {type_name}")
            }
            RegistrationError::UnknownOperation { at, operation } => {
                write!(f, "{at} refers to the unknown operation {operation}")
            }
            RegistrationError::RelationTargetNotQuery {
                type_name,
                field,
                operation,
            } => write!(
                f,
                "{type_name}.{field} is resolved by {operation}, which is not a query"
            ),
            RegistrationError::RelationArgMissing {
                type_name,
                field,
                arg,
            } => write!(
                f,
                "{type_name}.{field} fills {arg}, which is not an input of its operation"
            ),
            RegistrationError::RelationSourceMissing {
                type_name,
                field,
                source,
            } => write!(
                f,
                "{type_name}.{field} reads {source}, which is not a field of {type_name}"
            ),
            RegistrationError::RelationFieldCollides { type_name, field } => {
                write!(f, "{type_name}.{field} is both a field and a relation")
            }
            RegistrationError::Closed { name } => write!(
                f,
                "registration is closed; {name} differs from what is registered"
            ),
        }
    }
}

impl std::error::Error for RegistrationError {}
