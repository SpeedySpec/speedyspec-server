//! The `Type` builder.

use indexmap::IndexMap;
use speedyspec_operation_contracts::{Property, Relation, TypeSpec};

use crate::{S, SchemaBuilder};

/// Starts named resource types.
pub struct Type;

impl Type {
    pub fn resource(name: &str) -> TypeBuilder {
        TypeBuilder {
            spec: TypeSpec {
                name: name.to_owned(),
                fields: IndexMap::new(),
                relations: Vec::new(),
            },
        }
    }

    /// A schema referring to a named type.
    pub fn reference(name: &str) -> SchemaBuilder {
        S::reference(name)
    }
}

/// Builds one [`TypeSpec`].
pub struct TypeBuilder {
    spec: TypeSpec,
}

impl TypeBuilder {
    pub fn field(mut self, name: &str, schema: SchemaBuilder) -> Self {
        let required = schema.is_required();
        self.spec.fields.insert(
            name.to_owned(),
            Property {
                schema: schema.build(),
                required,
            },
        );
        self
    }

    /// `field` is resolved by `operation`, whose input fields (keys) are filled from this type's fields (values).
    pub fn relation<'a>(
        mut self,
        field: &str,
        operation: &str,
        args: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Self {
        self.spec.relations.push(Relation {
            field: field.to_owned(),
            operation: operation.to_owned(),
            args: args
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect(),
        });
        self
    }

    pub fn build(self) -> TypeSpec {
        self.spec
    }
}
