//! The identity map: one in-memory object per stored identity, for one unit of work.

use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use speedyspec_data::Identified;

/// Loaded entities by type and identity.
#[derive(Default)]
pub struct IdentityMap {
    maps: RefCell<HashMap<TypeId, Box<dyn Any>>>,
}

type Entries<E> = HashMap<<E as Identified>::Id, Rc<RefCell<E>>>;

impl IdentityMap {
    pub fn new() -> IdentityMap {
        IdentityMap::default()
    }

    pub fn get<E: Identified + 'static>(&self, id: &E::Id) -> Option<Rc<RefCell<E>>> {
        self.maps
            .borrow()
            .get(&TypeId::of::<E>())?
            .downcast_ref::<Entries<E>>()?
            .get(id)
            .cloned()
    }

    /// Keeps the entity; when its identity is already present, returns the existing entry instead.
    pub fn insert<E: Identified + 'static>(&self, entity: E) -> Rc<RefCell<E>> {
        let mut maps = self.maps.borrow_mut();
        let entries = maps
            .entry(TypeId::of::<E>())
            .or_insert_with(|| Box::new(Entries::<E>::new()))
            .downcast_mut::<Entries<E>>()
            .expect("entries are keyed by their own type");
        entries
            .entry(entity.identity())
            .or_insert_with(|| Rc::new(RefCell::new(entity)))
            .clone()
    }

    pub fn remove<E: Identified + 'static>(&self, id: &E::Id) {
        if let Some(entries) = self
            .maps
            .borrow_mut()
            .get_mut(&TypeId::of::<E>())
            .and_then(|m| m.downcast_mut::<Entries<E>>())
        {
            entries.remove(id);
        }
    }

    pub fn clear(&self) {
        self.maps.borrow_mut().clear();
    }
}
