//! An in-memory meta repository, for tests and for callers with no database.
//!
//! Rows live in insert order, which is id order because ids only grow. Keys and values are compared byte for byte,
//! and an update reports the rows it changed, as MySQL's affected-rows does without `CLIENT_FOUND_ROWS`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use speedyspec_wp_meta_contracts::{
    Meta, MetaError, MetaId, MetaKey, MetaRepositories, MetaRepository, MetaRow, MetaType,
    ObjectId, SerializedValue,
};

/// One meta table held in memory (Repository per meta type, Strategy for the engine).
pub struct MemoryMetaRepository {
    meta_type: MetaType,
    state: RefCell<State>,
}

struct State {
    rows: Vec<Meta>,
    last_id: u64,
}

impl MemoryMetaRepository {
    /// An empty table whose first id is 1.
    pub fn new(meta_type: MetaType) -> MemoryMetaRepository {
        MemoryMetaRepository {
            meta_type,
            state: RefCell::new(State {
                rows: Vec::new(),
                last_id: 0,
            }),
        }
    }
}

/// `meta_value = %s`: NULL never equals anything.
fn value_matches(row: &Option<SerializedValue>, wanted: Option<&SerializedValue>) -> bool {
    matches!((row, wanted), (Some(a), Some(b)) if a == b)
}

impl MetaRepository for MemoryMetaRepository {
    fn engine(&self) -> String {
        "memory".to_owned()
    }

    fn meta_type(&self) -> &MetaType {
        &self.meta_type
    }

    fn count_for(&self, object: ObjectId, key: &MetaKey) -> Result<u64, MetaError> {
        let state = self.state.borrow();
        Ok(state
            .rows
            .iter()
            .filter(|m| m.object_id == object && &m.key == key)
            .count() as u64)
    }

    fn add(
        &self,
        object: ObjectId,
        key: &MetaKey,
        value: Option<&SerializedValue>,
    ) -> Result<Option<MetaId>, MetaError> {
        let mut state = self.state.borrow_mut();
        state.last_id += 1;
        let mid = MetaId(state.last_id);
        state.rows.push(Meta {
            mid,
            object_id: object,
            key: key.clone(),
            value: value.cloned(),
        });
        Ok(Some(mid))
    }

    fn ids_for(&self, object: ObjectId, key: &MetaKey) -> Result<Vec<MetaId>, MetaError> {
        let state = self.state.borrow();
        Ok(state
            .rows
            .iter()
            .filter(|m| m.object_id == object && &m.key == key)
            .map(|m| m.mid)
            .collect())
    }

    fn update_by_key(
        &self,
        object: ObjectId,
        key: &MetaKey,
        value: Option<&SerializedValue>,
        previous: Option<&SerializedValue>,
    ) -> Result<u64, MetaError> {
        let mut state = self.state.borrow_mut();
        let mut changed = 0;
        for row in state
            .rows
            .iter_mut()
            .filter(|m| m.object_id == object && &m.key == key)
        {
            if previous.is_some() && !value_matches(&row.value, previous) {
                continue;
            }
            if row.value.as_ref() != value {
                row.value = value.cloned();
                changed += 1;
            }
        }
        Ok(changed)
    }

    fn ids_matching(
        &self,
        key: &MetaKey,
        object: Option<ObjectId>,
        value: Option<&SerializedValue>,
    ) -> Result<Vec<MetaId>, MetaError> {
        let state = self.state.borrow();
        Ok(state
            .rows
            .iter()
            .filter(|m| &m.key == key)
            .filter(|m| object.is_none_or(|o| m.object_id == o))
            .filter(|m| value.is_none() || value_matches(&m.value, value))
            .map(|m| m.mid)
            .collect())
    }

    fn objects_with(
        &self,
        key: &MetaKey,
        value: Option<&SerializedValue>,
    ) -> Result<Vec<ObjectId>, MetaError> {
        let state = self.state.borrow();
        Ok(state
            .rows
            .iter()
            .filter(|m| &m.key == key)
            .filter(|m| value.is_none() || value_matches(&m.value, value))
            .map(|m| m.object_id)
            .collect())
    }

    fn delete_ids(&self, ids: &[MetaId]) -> Result<u64, MetaError> {
        let mut state = self.state.borrow_mut();
        let before = state.rows.len();
        state.rows.retain(|m| !ids.contains(&m.mid));
        Ok((before - state.rows.len()) as u64)
    }

    fn by_mid(&self, mid: MetaId) -> Result<Option<Meta>, MetaError> {
        let state = self.state.borrow();
        Ok(state.rows.iter().find(|m| m.mid == mid).cloned())
    }

    fn update_by_mid(
        &self,
        mid: MetaId,
        key: &MetaKey,
        value: Option<&SerializedValue>,
    ) -> Result<u64, MetaError> {
        let mut state = self.state.borrow_mut();
        let Some(row) = state.rows.iter_mut().find(|m| m.mid == mid) else {
            return Ok(0);
        };
        if &row.key == key && row.value.as_ref() == value {
            return Ok(0);
        }
        row.key = key.clone();
        row.value = value.cloned();
        Ok(1)
    }

    fn delete_by_mid(&self, mid: MetaId) -> Result<u64, MetaError> {
        self.delete_ids(&[mid])
    }

    fn for_objects(&self, objects: &[ObjectId]) -> Result<Vec<MetaRow>, MetaError> {
        let state = self.state.borrow();
        Ok(state
            .rows
            .iter()
            .filter(|m| objects.contains(&m.object_id))
            .map(|m| MetaRow {
                object_id: m.object_id,
                key: m.key.clone(),
                value: m.value.clone(),
            })
            .collect())
    }

    fn ids_of_object(&self, object: ObjectId) -> Result<Vec<MetaId>, MetaError> {
        let state = self.state.borrow();
        Ok(state
            .rows
            .iter()
            .filter(|m| m.object_id == object)
            .map(|m| m.mid)
            .collect())
    }
}

/// One shared repository per meta type (Abstract Factory); `None` for a type it was not given.
pub struct MemoryMetaRepositories {
    repositories: HashMap<MetaType, Rc<dyn MetaRepository>>,
}

impl MemoryMetaRepositories {
    /// Names that are empty are skipped, as no such meta type exists.
    pub fn new(types: &[&str]) -> MemoryMetaRepositories {
        let repositories = types
            .iter()
            .filter_map(|name| MetaType::new(name))
            .map(|meta_type| {
                let repository: Rc<dyn MetaRepository> =
                    Rc::new(MemoryMetaRepository::new(meta_type.clone()));
                (meta_type, repository)
            })
            .collect();
        MemoryMetaRepositories { repositories }
    }
}

impl MetaRepositories for MemoryMetaRepositories {
    fn for_type(&self, meta_type: &MetaType) -> Option<Rc<dyn MetaRepository>> {
        self.repositories.get(meta_type).cloned()
    }
}
