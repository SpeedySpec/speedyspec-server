use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::rc::Rc;

/// Values attached to a server request by the steps it passes through, keyed by type.
///
/// PSR-7 keys attributes by string; keying by type lets a later step read what an earlier one stored (the routing
/// result, route parameters) without a cast that could fail at run time.
#[derive(Clone, Default)]
pub struct Attributes {
    values: HashMap<TypeId, Rc<dyn Any>>,
}

impl Attributes {
    pub fn new() -> Attributes {
        Attributes::default()
    }

    /// Stores `value`, replacing any earlier value of the same type.
    pub fn with<T: 'static>(mut self, value: T) -> Attributes {
        self.values.insert(TypeId::of::<T>(), Rc::new(value));
        self
    }

    pub fn get<T: 'static>(&self) -> Option<&T> {
        self.values
            .get(&TypeId::of::<T>())
            .and_then(|value| value.downcast_ref::<T>())
    }

    pub fn without<T: 'static>(mut self) -> Attributes {
        self.values.remove(&TypeId::of::<T>());
        self
    }
}

impl std::fmt::Debug for Attributes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Attributes({} values)", self.values.len())
    }
}
