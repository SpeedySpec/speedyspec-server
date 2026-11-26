//! Every `wp_cache_*` function as a use case on the buses (Command / CQRS), handled by [`CurrentObjectCache`].

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_cqrs::{
    Command, CommandHandler, Query, QueryHandler, bind_command_handler, bind_query_handler,
};
use speedyspec_hooks::{Key, Value};
use speedyspec_wp_object_cache_contracts::ObjectCacheError;

use crate::{CurrentObjectCache, WpObjectCache};

macro_rules! message {
    ($kind:ident $name:ident { $($field:ident : $ty:ty),* } -> $result:ty, |$c:ident, $m:ident| $body:expr) => {
        #[derive(Clone, Debug)]
        pub struct $name { $(pub $field: $ty),* }
        message!(@impl $kind $name $result);
        message!(@handle $kind $name $result, $c, $m, $body);
    };
    (@impl command $name:ident $result:ty) => {
        impl Command for $name {
            type Result = $result;
            const KEY: &'static str = concat!("SpeedySpec\\WpObjectCache\\", stringify!($name), "Handler");
        }
    };
    (@impl query $name:ident $result:ty) => {
        impl Query for $name {
            type Result = $result;
            const KEY: &'static str = concat!("SpeedySpec\\WpObjectCache\\", stringify!($name), "Handler");
        }
    };
    (@handle command $name:ident $result:ty, $c:ident, $m:ident, $body:expr) => {
        impl CommandHandler<$name> for CurrentObjectCache {
            #[allow(unused_variables)]
            fn handle(&self, $m: $name) -> $result {
                let $c: Rc<WpObjectCache> = self.current();
                $body
            }
        }
    };
    (@handle query $name:ident $result:ty, $c:ident, $m:ident, $body:expr) => {
        impl QueryHandler<$name> for CurrentObjectCache {
            #[allow(unused_variables)]
            fn handle(&self, $m: $name) -> $result {
                let $c: Rc<WpObjectCache> = self.current();
                $body
            }
        }
    };
}

type R<T> = Result<T, ObjectCacheError>;

message!(command CacheAdd { key: Value, data: Value, group: Value, expire: i64 } -> R<bool>, |c, m| c.add(&m.key, m.data, &m.group, m.expire));
message!(command CacheAddMultiple { data: Vec<(Key, Value)>, group: Value, expire: i64 } -> R<Vec<(Key, bool)>>, |c, m| c.add_multiple(m.data, &m.group, m.expire));
message!(command CacheReplace { key: Value, data: Value, group: Value, expire: i64 } -> R<bool>, |c, m| c.replace(&m.key, m.data, &m.group, m.expire));
message!(command CacheSet { key: Value, data: Value, group: Value, expire: i64 } -> R<bool>, |c, m| c.set(&m.key, m.data, &m.group, m.expire));
message!(command CacheSetMultiple { data: Vec<(Key, Value)>, group: Value, expire: i64 } -> R<Vec<(Key, bool)>>, |c, m| c.set_multiple(m.data, &m.group, m.expire));
message!(command CacheDelete { key: Value, group: Value } -> R<bool>, |c, m| c.delete(&m.key, &m.group));
message!(command CacheDeleteMultiple { keys: Vec<Value>, group: Value } -> R<Vec<(Key, bool)>>, |c, m| c.delete_multiple(&m.keys, &m.group));
message!(command CacheIncr { key: Value, offset: i64, group: Value } -> R<Option<Value>>, |c, m| c.incr(&m.key, m.offset, &m.group));
message!(command CacheDecr { key: Value, offset: i64, group: Value } -> R<Option<Value>>, |c, m| c.decr(&m.key, m.offset, &m.group));
message!(command CacheFlush {} -> bool, |c, m| c.flush());
message!(command CacheFlushRuntime {} -> bool, |c, m| c.flush_runtime());
message!(command CacheFlushGroup { group: Value } -> R<bool>, |c, m| c.flush_group(&m.group));
message!(command CacheAddGlobalGroups { groups: Vec<Value> } -> (), |c, m| c.add_global_groups(&m.groups));
message!(command CacheAddNonPersistentGroups { groups: Vec<Value> } -> (), |c, m| c.add_non_persistent_groups(&m.groups));
message!(command CacheSwitchToBlog { blog_id: i64 } -> (), |c, m| c.switch_to_blog(m.blog_id));
message!(command CacheReset {} -> (), |c, m| c.reset());
message!(query CacheGet { key: Value, group: Value, force: bool } -> R<Option<Value>>, |c, m| c.get(&m.key, &m.group, m.force));
message!(query CacheGetMultiple { keys: Vec<Value>, group: Value, force: bool } -> R<Vec<(Key, Value)>>, |c, m| c.get_multiple(&m.keys, &m.group, m.force));
message!(query CacheSupports { feature: String } -> bool, |c, m| WpObjectCache::supports(&m.feature));

macro_rules! bind_all {
    ($container:expr, $api:expr; commands: $($c:ty),*; queries: $($q:ty),*) => {
        $( { let a = $api.clone(); bind_command_handler::<$c>($container, move |_, _| Ok(a.clone())); } )*
        $( { let a = $api.clone(); bind_query_handler::<$q>($container, move |_, _| Ok(a.clone())); } )*
    };
}

/// Binds `current` as the handler of every object-cache message; PHP overrides one by binding the same key.
pub fn register_object_cache_handlers(container: &Container, current: Rc<CurrentObjectCache>) {
    bind_all!(container, current;
        commands: CacheAdd, CacheAddMultiple, CacheReplace, CacheSet, CacheSetMultiple, CacheDelete,
            CacheDeleteMultiple, CacheIncr, CacheDecr, CacheFlush, CacheFlushRuntime, CacheFlushGroup,
            CacheAddGlobalGroups, CacheAddNonPersistentGroups, CacheSwitchToBlog, CacheReset;
        queries: CacheGet, CacheGetMultiple, CacheSupports);
}
