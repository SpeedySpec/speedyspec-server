//! The second level: what the first level does when the cache has a persistent `CachingStore` and the group is not
//! non-persistent. Every failure of the second level is a miss (or a first-level-only write) with a notice; none
//! reaches the caller.

use std::rc::Rc;
use std::time::Duration;

use speedyspec_cache::CachingStore;
use speedyspec_hooks::{Key, Value};

use crate::keys::{array_key, key_text};
use crate::{CacheValue, CodecError, ObjectCache, ObjectCacheError};

fn expiry(expire: i64) -> Option<Duration> {
    (expire > 0).then(|| Duration::from_secs(expire as u64))
}

impl ObjectCache {
    /// The second level for this group, if it has one.
    fn second(&self, group: &Key) -> Option<&Rc<dyn CachingStore>> {
        self.persistent
            .as_ref()
            .filter(|_| !self.non_persistent.borrow().contains(group))
    }

    pub(crate) fn persists(&self, group: &Key) -> bool {
        self.second(group).is_some()
    }

    fn miss(&self, group: &Key, id: &Key, why: &str) {
        self.ports
            .notices
            .persistence_miss(&key_text(group), &key_text(id), why);
    }

    /// Writes the first level, and the second when there is one; a value the codec refuses stays first-level.
    pub(crate) fn write(&self, id: Key, group: Key, data: Value, expire: i64, _overwrite: bool) {
        if let Some(store) = self.second(&group) {
            match self.ports.codec.encode(&data) {
                Ok(bytes) => {
                    if let Err(e) =
                        store.set(&key_text(&group), &key_text(&id), &bytes, expiry(expire))
                    {
                        self.miss(&group, &id, &e.to_string());
                        self.forget_second(&id, &group);
                    }
                }
                Err(CodecError::Refused(why) | CodecError::Corrupt(why)) => {
                    self.miss(&group, &id, &why);
                    // The value the caller replaced must not come back in a later request.
                    self.forget_second(&id, &group);
                }
            }
        }
        self.store(id, group, CacheValue::Php(data));
    }

    /// The second level's `add`: `None` without one (or when the value stays first-level), else whether it
    /// added the entry (false when another writer holds the key).
    pub(crate) fn second_add(&self, id: &Key, g: &Key, data: &Value, expire: i64) -> Option<bool> {
        let store = self.second(g)?;
        let bytes = match self.ports.codec.encode(data) {
            Ok(bytes) => bytes,
            Err(CodecError::Refused(why) | CodecError::Corrupt(why)) => {
                self.miss(g, id, &why);
                return None;
            }
        };
        match store.add(&key_text(g), &key_text(id), &bytes, expiry(expire)) {
            Ok(added) => Some(added),
            Err(e) => {
                self.miss(g, id, &e.to_string());
                None
            }
        }
    }

    pub(crate) fn second_has(&self, id: &Key, g: &Key) -> bool {
        let Some(store) = self.second(g) else {
            return false;
        };
        match store.get(&key_text(g), &key_text(id)) {
            Ok(found) => found.is_some(),
            Err(e) => {
                self.miss(g, id, &e.to_string());
                false
            }
        }
    }

    /// Reads the second level, keeping a hit in the first level as bytes decoded on return.
    pub(crate) fn second_get(&self, id: &Key, g: &Key) -> Option<Value> {
        let store = self.second(g)?;
        match store.get(&key_text(g), &key_text(id)) {
            Ok(Some(bytes)) => {
                self.store(id.clone(), g.clone(), CacheValue::Bytes(bytes));
                self.decode_entry(id, g)
            }
            Ok(None) => None,
            Err(e) => {
                self.miss(g, id, &e.to_string());
                None
            }
        }
    }

    /// Removes the second level's entry, best effort: a value it cannot hold, or bytes it holds that cannot be read.
    pub(crate) fn forget_second(&self, id: &Key, g: &Key) {
        if let Some(store) = self.second(g)
            && let Err(e) = store.delete(&key_text(g), &key_text(id))
        {
            self.miss(g, id, &e.to_string());
        }
    }

    pub(crate) fn second_delete(&self, id: &Key, g: &Key) -> bool {
        let Some(store) = self.second(g) else {
            return false;
        };
        store
            .delete(&key_text(g), &key_text(id))
            .unwrap_or_else(|e| {
                self.miss(g, id, &e.to_string());
                false
            })
    }

    pub(crate) fn second_flush(&self) {
        if let Some(store) = &self.persistent
            && let Err(e) = store.flush()
        {
            self.ports.notices.persistence_miss("", "", &e.to_string());
        }
    }

    pub(crate) fn second_flush_group(&self, g: &Key) {
        if let Some(store) = self.second(g)
            && let Err(e) = store.flush_group(&key_text(g))
        {
            self.miss(g, &Key::Str(String::new()), &e.to_string());
        }
    }

    /// `get_multiple`: every first-level miss read from the second level with one `get_many`.
    pub(crate) fn prefetch(
        &self,
        keys: &[Value],
        group: &Value,
        force: bool,
    ) -> Result<(), ObjectCacheError> {
        let g = if crate::cache::empty_group(group) {
            Key::Str("default".into())
        } else {
            array_key(group)?
        };
        let Some(store) = self.second(&g).cloned() else {
            return Ok(());
        };
        let ids: Vec<Key> = keys
            .iter()
            .filter_map(|k| array_key(k).ok())
            .map(|k| self.prefixed(&k, &g))
            .filter(|id| force || !self.has_entry(id, &g))
            .collect();
        if ids.is_empty() {
            return Ok(());
        }
        let texts: Vec<String> = ids.iter().map(key_text).collect();
        let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
        match store.get_many(&key_text(&g), &refs) {
            Ok(found) => {
                for (id, bytes) in ids.into_iter().zip(found) {
                    if let Some(bytes) = bytes {
                        self.store(id, g.clone(), CacheValue::Bytes(bytes));
                    }
                }
            }
            Err(e) => self.miss(&g, &Key::Str(String::new()), &e.to_string()),
        }
        Ok(())
    }
}
