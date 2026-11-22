//! The in-memory type and operation registries.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use regex::Regex;
use serde_json::{Map, Value as Json, json};
use speedyspec_operation_contracts::{
    Kind, OperationCatalog, OperationSpec, Schema, SchemaKind, TypeCatalog, TypeSpec,
};

use crate::RegistrationError;
use crate::export::{operation_document, type_document};

/// The registered types and operations (Registry pattern). Registration is open until [`Registries::close`], which runs
/// the checks that need everything registered; after that, re-registering an identical definition is a no-op and
/// anything else is refused, so legacy code that registers on every request keeps working.
#[derive(Default)]
pub struct Registries {
    types: RefCell<BTreeMap<String, Rc<TypeSpec>>>,
    operations: RefCell<BTreeMap<String, Rc<OperationSpec>>>,
    closed: Cell<bool>,
}

impl Registries {
    pub fn new() -> Self {
        Registries::default()
    }

    pub fn register_type(&self, spec: TypeSpec) -> Result<(), RegistrationError> {
        if self.closed.get() {
            let same = self
                .types
                .borrow()
                .get(&spec.name)
                .is_some_and(|existing| type_document(existing) == type_document(&spec));
            return if same {
                Ok(())
            } else {
                Err(RegistrationError::Closed { name: spec.name })
            };
        }
        if self.types.borrow().contains_key(&spec.name) {
            return Err(RegistrationError::Duplicate { name: spec.name });
        }
        for (field, property) in &spec.fields {
            check_patterns(&property.schema, &format!("{}.{field}", spec.name))?;
        }
        self.types
            .borrow_mut()
            .insert(spec.name.clone(), Rc::new(spec));
        Ok(())
    }

    pub fn register_operation(&self, spec: OperationSpec) -> Result<(), RegistrationError> {
        let name = spec.name.as_str().to_owned();
        if self.closed.get() {
            let same =
                self.operations.borrow().get(&name).is_some_and(|existing| {
                    operation_document(existing) == operation_document(&spec)
                });
            return if same {
                Ok(())
            } else {
                Err(RegistrationError::Closed { name })
            };
        }
        if self.operations.borrow().contains_key(&name) {
            return Err(RegistrationError::Duplicate { name });
        }
        check_patterns(&spec.input.schema, &format!("{name}.input"))?;
        check_patterns(&spec.output.schema, &format!("{name}.output"))?;
        self.operations.borrow_mut().insert(name, Rc::new(spec));
        Ok(())
    }

    /// Limits the operation `name` to the named transports. After closing, the same restriction is a no-op and any
    /// other is refused.
    pub fn restrict(&self, name: &str, transports: &[&str]) -> Result<(), RegistrationError> {
        let Some(existing) = self.operations.borrow().get(name).cloned() else {
            return Err(RegistrationError::UnknownOperation {
                at: format!("{name}.transports"),
                operation: name.to_owned(),
            });
        };
        let wanted: BTreeSet<String> = transports.iter().map(|t| (*t).to_owned()).collect();
        if self.closed.get() {
            return if existing.transports.as_ref() == Some(&wanted) {
                Ok(())
            } else {
                Err(RegistrationError::Closed {
                    name: name.to_owned(),
                })
            };
        }
        let mut restricted = (*existing).clone();
        restricted.transports = Some(wanted);
        self.operations
            .borrow_mut()
            .insert(name.to_owned(), Rc::new(restricted));
        Ok(())
    }

    /// Checks every reference and relation, collecting all failures. Closes registration only when there are none.
    pub fn close(&self) -> Result<(), Vec<RegistrationError>> {
        let errors = self.check();
        if errors.is_empty() {
            self.closed.set(true);
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Closes registration whatever the checks find: every operation or type an error names is dropped, and the
    /// checks run again until the rest passes. Returns every error seen, so one plugin's broken reference costs only
    /// its own definitions.
    pub fn close_lenient(&self) -> Vec<RegistrationError> {
        let mut seen = Vec::new();
        loop {
            let errors = self.check();
            if errors.is_empty() {
                break;
            }
            let mut dropped = false;
            for owner in errors.iter().filter_map(owner_of) {
                dropped |= self.operations.borrow_mut().remove(&owner).is_some();
                dropped |= self.types.borrow_mut().remove(&owner).is_some();
            }
            seen.extend(errors);
            if !dropped {
                break;
            }
        }
        self.closed.set(true);
        seen
    }

    fn check(&self) -> Vec<RegistrationError> {
        let mut errors = Vec::new();
        let types = self.types.borrow();
        let operations = self.operations.borrow();
        for spec in types.values() {
            for (field, property) in &spec.fields {
                check_refs(
                    &property.schema,
                    &format!("{}.{field}", spec.name),
                    &types,
                    &mut errors,
                );
            }
            for relation in &spec.relations {
                let at = format!("{}.{}", spec.name, relation.field);
                if spec.fields.contains_key(&relation.field) {
                    errors.push(RegistrationError::RelationFieldCollides {
                        type_name: spec.name.clone(),
                        field: relation.field.clone(),
                    });
                }
                for source in relation
                    .args
                    .values()
                    .filter(|s| !spec.fields.contains_key(*s))
                {
                    errors.push(RegistrationError::RelationSourceMissing {
                        type_name: spec.name.clone(),
                        field: relation.field.clone(),
                        source: source.clone(),
                    });
                }
                let Some(target) = operations.get(&relation.operation) else {
                    errors.push(RegistrationError::UnknownOperation {
                        at,
                        operation: relation.operation.clone(),
                    });
                    continue;
                };
                if target.kind != Kind::Query {
                    errors.push(RegistrationError::RelationTargetNotQuery {
                        type_name: spec.name.clone(),
                        field: relation.field.clone(),
                        operation: relation.operation.clone(),
                    });
                }
                let inputs = object_properties(&target.input.schema, &types);
                for arg in relation
                    .args
                    .keys()
                    .filter(|a| !inputs.is_some_and(|p| p.contains_key(*a)))
                {
                    errors.push(RegistrationError::RelationArgMissing {
                        type_name: spec.name.clone(),
                        field: relation.field.clone(),
                        arg: arg.clone(),
                    });
                }
            }
        }
        for (name, spec) in operations.iter() {
            check_refs(
                &spec.input.schema,
                &format!("{name}.input"),
                &types,
                &mut errors,
            );
            check_refs(
                &spec.output.schema,
                &format!("{name}.output"),
                &types,
                &mut errors,
            );
            if let Some(resource) = &spec.resource {
                if !types.contains_key(&resource.type_name) {
                    errors.push(RegistrationError::UnknownType {
                        at: format!("{name}.resource"),
                        type_name: resource.type_name.clone(),
                    });
                }
                let inputs = object_properties(&spec.input.schema, &types);
                for field in resource
                    .identity
                    .iter()
                    .filter(|f| !inputs.is_some_and(|p| p.contains_key(*f)))
                {
                    errors.push(RegistrationError::UnknownIdentityField {
                        operation: name.clone(),
                        field: field.clone(),
                    });
                }
            }
        }
        errors
    }

    pub fn is_closed(&self) -> bool {
        self.closed.get()
    }

    /// Every type and operation as one JSON document (JSON Schema 2020-12 plus operation traits), ordered by name.
    pub fn document(&self) -> Json {
        let types: Map<String, Json> = self
            .types
            .borrow()
            .iter()
            .map(|(name, spec)| (name.clone(), type_document(spec)))
            .collect();
        let operations: Map<String, Json> = self
            .operations
            .borrow()
            .iter()
            .map(|(name, spec)| (name.clone(), operation_document(spec)))
            .collect();
        json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "types": types,
            "operations": operations,
        })
    }
}

impl TypeCatalog for Registries {
    fn get_type(&self, name: &str) -> Option<Rc<TypeSpec>> {
        self.types.borrow().get(name).cloned()
    }

    fn type_names(&self) -> Vec<String> {
        self.types.borrow().keys().cloned().collect()
    }
}

impl OperationCatalog for Registries {
    fn get_operation(&self, name: &str) -> Option<Rc<OperationSpec>> {
        self.operations.borrow().get(name).cloned()
    }

    fn operation_names(&self) -> Vec<String> {
        self.operations.borrow().keys().cloned().collect()
    }
}

/// The operation or type a close-time error belongs to. Paths start with the owner's name; neither operation nor
/// type names contain dots.
fn owner_of(error: &RegistrationError) -> Option<String> {
    let path = |at: &str| at.split('.').next().unwrap_or(at).to_owned();
    Some(match error {
        RegistrationError::UnknownType { at, .. }
        | RegistrationError::UnknownOperation { at, .. } => path(at),
        RegistrationError::RelationTargetNotQuery { type_name, .. }
        | RegistrationError::RelationArgMissing { type_name, .. }
        | RegistrationError::RelationSourceMissing { type_name, .. }
        | RegistrationError::RelationFieldCollides { type_name, .. } => type_name.clone(),
        RegistrationError::UnknownIdentityField { operation, .. } => operation.clone(),
        _ => return None,
    })
}

/// Calls `visit` on `schema` and every schema nested in it, with its path.
fn walk(schema: &Schema, at: &str, visit: &mut dyn FnMut(&Schema, &str)) {
    visit(schema, at);
    match &schema.kind {
        SchemaKind::Array { items, .. } => walk(items, &format!("{at}[]"), visit),
        SchemaKind::Object { properties, .. } => {
            for (name, property) in properties {
                walk(&property.schema, &format!("{at}.{name}"), visit);
            }
        }
        SchemaKind::OneOf { variants, .. } => {
            for (i, variant) in variants.iter().enumerate() {
                walk(variant, &format!("{at}.oneOf[{i}]"), visit);
            }
        }
        _ => {}
    }
}

fn check_patterns(schema: &Schema, at: &str) -> Result<(), RegistrationError> {
    let mut failure = None;
    walk(schema, at, &mut |s, path| {
        if let SchemaKind::String {
            pattern: Some(p), ..
        } = &s.kind
            && failure.is_none()
            && let Err(e) = Regex::new(p)
        {
            failure = Some(RegistrationError::InvalidSchema {
                at: path.to_owned(),
                why: e.to_string(),
            });
        }
    });
    failure.map_or(Ok(()), Err)
}

fn check_refs(
    schema: &Schema,
    at: &str,
    types: &BTreeMap<String, Rc<TypeSpec>>,
    errors: &mut Vec<RegistrationError>,
) {
    walk(schema, at, &mut |s, path| {
        if let SchemaKind::Ref { type_name } = &s.kind
            && !types.contains_key(type_name)
        {
            errors.push(RegistrationError::UnknownType {
                at: path.to_owned(),
                type_name: type_name.clone(),
            });
        }
    });
}

/// The properties of an object schema, or of the named type a `Ref` points at.
fn object_properties<'a>(
    schema: &'a Schema,
    types: &'a BTreeMap<String, Rc<TypeSpec>>,
) -> Option<&'a indexmap::IndexMap<String, speedyspec_operation_contracts::Property>> {
    match &schema.kind {
        SchemaKind::Object { properties, .. } => Some(properties),
        SchemaKind::Ref { type_name } => types.get(type_name).map(|t| &t.fields),
        _ => None,
    }
}
