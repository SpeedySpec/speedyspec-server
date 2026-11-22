//! The `Operation` builder.

use std::any::Any;
use std::collections::{BTreeSet, HashSet};
use std::rc::Rc;

use speedyspec_operation_contracts::{
    Action, Caller, Category, ContractMapper, ContractSpec, ErrorSpec, Extract, Hydrate, Kind,
    Labels, OperationError, OperationName, OperationSpec, Pagination, PermissionCheck, Resource,
};

use crate::{InputMapper, OutputMapper, RegistrationError, SchemaBuilder};

/// Starts operations. The kind is fixed by the starting function, so a read cannot be marked destructive by accident.
pub struct Operation;

impl Operation {
    pub fn query(name: &str) -> OperationBuilder {
        OperationBuilder::new(name, Kind::Query)
    }

    pub fn command(name: &str) -> OperationBuilder {
        OperationBuilder::new(name, Kind::Command)
    }

    pub fn subscription(name: &str) -> OperationBuilder {
        OperationBuilder::new(name, Kind::Subscription)
    }
}

/// Builds one [`OperationSpec`]; `build()` checks it.
pub struct OperationBuilder {
    name: String,
    kind: Kind,
    resource: Option<Resource>,
    idempotent: bool,
    destructive: bool,
    pagination: Option<Pagination>,
    input: Option<ContractSpec>,
    output: Option<ContractSpec>,
    errors: Vec<ErrorSpec>,
    labels: Option<Labels>,
    deprecated: Option<String>,
    transports: Option<BTreeSet<String>>,
    permission: Option<Rc<dyn PermissionCheck>>,
}

impl OperationBuilder {
    fn new(name: &str, kind: Kind) -> Self {
        OperationBuilder {
            name: name.to_owned(),
            kind,
            resource: None,
            idempotent: false,
            destructive: false,
            pagination: None,
            input: None,
            output: None,
            errors: Vec::new(),
            labels: None,
            deprecated: None,
            transports: None,
            permission: None,
        }
    }

    pub fn resource(mut self, type_name: &str, action: Action, identity: &[&str]) -> Self {
        self.resource = Some(Resource {
            type_name: type_name.to_owned(),
            action,
            identity: identity.iter().map(|s| (*s).to_owned()).collect(),
        });
        self
    }

    pub fn idempotent(mut self) -> Self {
        self.idempotent = true;
        self
    }

    pub fn destructive(mut self) -> Self {
        self.destructive = true;
        self
    }

    pub fn paginated(mut self, pagination: Pagination) -> Self {
        self.pagination = Some(pagination);
        self
    }

    pub fn input<I: Hydrate>(self, schema: SchemaBuilder) -> Self {
        self.input_with(schema, Rc::new(InputMapper::<I>::new()))
    }

    pub fn output<O: Extract>(self, schema: SchemaBuilder) -> Self {
        self.output_with(schema, Rc::new(OutputMapper::<O>::new()))
    }

    pub fn input_with(mut self, schema: SchemaBuilder, mapper: Rc<dyn ContractMapper>) -> Self {
        self.input = Some(ContractSpec {
            schema: schema.build(),
            mapper,
        });
        self
    }

    pub fn output_with(mut self, schema: SchemaBuilder, mapper: Rc<dyn ContractMapper>) -> Self {
        self.output = Some(ContractSpec {
            schema: schema.build(),
            mapper,
        });
        self
    }

    pub fn error(mut self, code: &str, category: Category) -> Self {
        self.errors.push(ErrorSpec {
            code: code.to_owned(),
            category,
        });
        self
    }

    /// Allows the call when `check` returns true. A refusal is `unauthenticated` for anonymous callers and
    /// `permission_denied` otherwise, as WordPress REST answers 401 or 403.
    pub fn permission<I: 'static>(self, check: impl Fn(&Caller, &I) -> bool + 'static) -> Self {
        self.permission_check(Rc::new(TypedPermission(Box::new(check))))
    }

    pub fn permission_check(mut self, check: Rc<dyn PermissionCheck>) -> Self {
        self.permission = Some(check);
        self
    }

    /// Anyone may call it.
    pub fn public(self) -> Self {
        self.permission_check(Rc::new(Public))
    }

    pub fn label(mut self, title: &str, description: &str) -> Self {
        self.labels = Some(Labels {
            title: title.to_owned(),
            description: description.to_owned(),
        });
        self
    }

    pub fn deprecated(mut self, reason: &str) -> Self {
        self.deprecated = Some(reason.to_owned());
        self
    }

    /// Only these transports may expose it. Without this call, every transport does.
    pub fn transports(mut self, names: &[&str]) -> Self {
        self.transports = Some(names.iter().map(|s| (*s).to_owned()).collect());
        self
    }

    pub fn build(self) -> Result<OperationSpec, RegistrationError> {
        let name = OperationName::parse(&self.name)
            .map_err(|e| RegistrationError::InvalidName { name: e.name })?;
        let operation = self.name.clone();
        let input = self
            .input
            .ok_or_else(|| RegistrationError::MissingContract {
                operation: operation.clone(),
                which: "input",
            })?;
        let output = self
            .output
            .ok_or_else(|| RegistrationError::MissingContract {
                operation: operation.clone(),
                which: "output",
            })?;
        let permission = self
            .permission
            .ok_or_else(|| RegistrationError::MissingPermission {
                operation: operation.clone(),
            })?;
        let mut seen = HashSet::new();
        if let Some(dup) = self.errors.iter().find(|e| !seen.insert(e.code.as_str())) {
            return Err(RegistrationError::DuplicateErrorCode {
                operation,
                code: dup.code.clone(),
            });
        }
        if self.kind == Kind::Query && self.destructive {
            return Err(RegistrationError::DestructiveQuery { operation });
        }
        Ok(OperationSpec {
            labels: self.labels.unwrap_or_else(|| Labels {
                title: operation.clone(),
                description: String::new(),
            }),
            name,
            kind: self.kind,
            resource: self.resource,
            idempotent: self.idempotent,
            destructive: self.destructive,
            pagination: self.pagination,
            input,
            output,
            errors: self.errors,
            deprecated: self.deprecated,
            transports: self.transports,
            permission,
        })
    }
}

type Check<I> = Box<dyn Fn(&Caller, &I) -> bool>;

struct TypedPermission<I>(Check<I>);

impl<I: 'static> PermissionCheck for TypedPermission<I> {
    fn check(&self, caller: &Caller, input: &dyn Any) -> Result<(), OperationError> {
        let input = input.downcast_ref::<I>().ok_or_else(|| {
            OperationError::new(
                "input_type_mismatch",
                Category::Internal,
                format!(
                    "the permission check expected a {}",
                    std::any::type_name::<I>()
                ),
            )
        })?;
        if (self.0)(caller, input) {
            Ok(())
        } else if caller.user.is_none() {
            Err(OperationError::new(
                "operation_unauthenticated",
                Category::Unauthenticated,
                "You must be signed in to do that.",
            ))
        } else {
            Err(OperationError::new(
                "operation_forbidden",
                Category::PermissionDenied,
                "You are not allowed to do that.",
            ))
        }
    }
}

struct Public;

impl PermissionCheck for Public {
    fn check(&self, _: &Caller, _: &dyn Any) -> Result<(), OperationError> {
        Ok(())
    }
}
