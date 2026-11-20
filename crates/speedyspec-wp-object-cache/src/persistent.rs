//! The second level (filled by the persistent level's task): with no persistent store every call is a no-op.

use speedyspec_hooks::{Key, Value};

use crate::{CacheValue, ObjectCache};

impl ObjectCache {
    pub(crate) fn persists(&self, _group: &Key) -> bool {
        false
    }
    pub(crate) fn write(&self, id: Key, group: Key, data: Value, _expire: i64, _overwrite: bool) {
        self.store(id, group, CacheValue::Php(data));
    }
    pub(crate) fn second_add(&self, _id: &Key, _g: &Key, _data: &Value, _expire: i64) -> bool {
        true
    }
    pub(crate) fn second_has(&self, _id: &Key, _g: &Key) -> bool {
        false
    }
    pub(crate) fn second_get(&self, _id: &Key, _g: &Key) -> Option<Value> {
        None
    }
    pub(crate) fn second_delete(&self, _id: &Key, _g: &Key) -> bool {
        false
    }
    pub(crate) fn second_flush(&self) {}
    pub(crate) fn second_flush_group(&self, _g: &Key) {}
    pub(crate) fn prefetch(
        &self,
        _keys: &[Value],
        _group: &Value,
        _force: bool,
    ) -> Result<(), crate::ObjectCacheError> {
        Ok(())
    }
}
