//! Ports for reading the registries.

use std::rc::Rc;

use crate::TypeSpec;

/// Reads registered types.
pub trait TypeCatalog {
    fn get_type(&self, name: &str) -> Option<Rc<TypeSpec>>;
    fn type_names(&self) -> Vec<String>;
}
