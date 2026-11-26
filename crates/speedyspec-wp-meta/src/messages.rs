//! Every metadata function as a use case on the buses (Command / CQRS), handled by [`MetadataApi`].

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_cqrs::{
    Command, CommandHandler, Query, QueryHandler, bind_command_handler, bind_query_handler,
};
use speedyspec_php_serialize::PhpValue;
use speedyspec_wp_meta_contracts::{
    MetaId, MetaKey, MetaType, MetadataError, ObjectId, UpdateOutcome,
};

use crate::{
    AddOutcome, ByMidOutcome, FilteredProtection, MetadataApi, object_subtype, sanitize_meta,
};

macro_rules! message {
    ($kind:ident $name:ident { $($field:ident : $ty:ty),* } -> $result:ty, |$a:ident, $m:ident| $body:expr) => {
        #[derive(Clone, Debug)]
        pub struct $name { $(pub $field: $ty),* }
        message!(@impl $kind $name $result);
        message!(@handle $kind $name $result, $a, $m, $body);
    };
    (@impl command $name:ident $result:ty) => {
        impl Command for $name {
            type Result = $result;
            const KEY: &'static str = concat!("SpeedySpec\\WpMeta\\", stringify!($name), "Handler");
        }
    };
    (@impl query $name:ident $result:ty) => {
        impl Query for $name {
            type Result = $result;
            const KEY: &'static str = concat!("SpeedySpec\\WpMeta\\", stringify!($name), "Handler");
        }
    };
    (@handle command $name:ident $result:ty, $a:ident, $m:ident, $body:expr) => {
        impl CommandHandler<$name> for MetadataApi {
            #[allow(unused_variables)]
            fn handle(&self, $m: $name) -> $result {
                let $a = self;
                $body
            }
        }
    };
    (@handle query $name:ident $result:ty, $a:ident, $m:ident, $body:expr) => {
        impl QueryHandler<$name> for MetadataApi {
            #[allow(unused_variables)]
            fn handle(&self, $m: $name) -> $result {
                let $a = self;
                $body
            }
        }
    };
}

type R<T> = Result<T, MetadataError>;

message!(command AddMetadata { meta_type: MetaType, object: ObjectId, key: MetaKey, value: PhpValue, unique: bool }
    -> R<AddOutcome>, |a, m| a.add_metadata(&m.meta_type, m.object, &m.key, m.value, m.unique));
message!(command UpdateMetadata { meta_type: MetaType, object: ObjectId, key: MetaKey, value: PhpValue,
    prev: Option<PhpValue> } -> R<UpdateOutcome>,
    |a, m| a.update_metadata(&m.meta_type, m.object, &m.key, m.value, m.prev));
message!(command DeleteMetadata { meta_type: MetaType, object: ObjectId, key: MetaKey, value: Option<PhpValue>,
    delete_all: bool } -> R<bool>,
    |a, m| a.delete_metadata(&m.meta_type, m.object, &m.key, m.value, m.delete_all));
message!(query GetMetadata { meta_type: MetaType, object: ObjectId, key: Option<MetaKey>, single: bool }
    -> R<PhpValue>, |a, m| a.get_metadata(&m.meta_type, m.object, m.key.as_ref(), m.single));
message!(query GetMetadataRaw { meta_type: MetaType, object: ObjectId, key: Option<MetaKey>, single: bool }
    -> R<Option<PhpValue>>, |a, m| a.get_metadata_raw(&m.meta_type, m.object, m.key.as_ref(), m.single));
message!(query GetMetadataDefault { meta_type: MetaType, object: ObjectId, key: MetaKey, single: bool }
    -> R<PhpValue>, |a, m| a.get_metadata_default(&m.meta_type, m.object, &m.key, m.single));
message!(query MetadataExists { meta_type: MetaType, object: ObjectId, key: MetaKey } -> R<bool>,
    |a, m| a.metadata_exists(&m.meta_type, m.object, &m.key));
message!(query GetMetadataByMid { meta_type: MetaType, mid: MetaId } -> R<ByMidOutcome>,
    |a, m| a.get_metadata_by_mid(&m.meta_type, m.mid));
message!(command UpdateMetadataByMid { meta_type: MetaType, mid: MetaId, value: PhpValue, key: Option<MetaKey> }
    -> R<bool>, |a, m| a.update_metadata_by_mid(&m.meta_type, m.mid, m.value, m.key.as_ref()));
message!(command DeleteMetadataByMid { meta_type: MetaType, mid: MetaId } -> R<bool>,
    |a, m| a.delete_metadata_by_mid(&m.meta_type, m.mid));
message!(command UpdateMetaCache { meta_type: MetaType, objects: Vec<ObjectId> } -> R<Option<PhpValue>>,
    |a, m| a.update_meta_cache(&m.meta_type, &m.objects));
message!(query SanitizeMeta { key: MetaKey, value: PhpValue, object_type: String, subtype: String } -> R<PhpValue>,
    |a, m| sanitize_meta(&*a.d.hooks, &m.key, m.value, &m.object_type, &m.subtype));
message!(query IsProtectedMeta { key: MetaKey, meta_type: String } -> R<bool>,
    |a, m| FilteredProtection::new(a.d.hooks.clone(), a.d.protection.clone()).try_is_protected(&m.key, &m.meta_type));
message!(query GetObjectSubtype { meta_type: MetaType, object: ObjectId } -> R<String>,
    |a, m| object_subtype(&*a.d.hooks, &*a.d.subtypes, &m.meta_type, m.object));
message!(query FilterDefaultMetadata { value: PhpValue, object: ObjectId, key: MetaKey, single: bool,
    meta_type: MetaType } -> R<PhpValue>,
    |a, m| a.filter_default_metadata(m.value, m.object, &m.key, m.single, &m.meta_type));

macro_rules! bind_all {
    ($container:expr, $api:expr; commands: $($c:ty),*; queries: $($q:ty),*) => {
        $( { let a = $api.clone(); bind_command_handler::<$c>($container, move |_, _| Ok(a.clone())); } )*
        $( { let a = $api.clone(); bind_query_handler::<$q>($container, move |_, _| Ok(a.clone())); } )*
    };
}

/// Binds `api` as the handler of every metadata message; PHP overrides one by binding the same key.
pub fn register_meta_handlers(container: &Container, api: Rc<MetadataApi>) {
    bind_all!(container, api;
        commands: AddMetadata, UpdateMetadata, DeleteMetadata, UpdateMetadataByMid, DeleteMetadataByMid,
            UpdateMetaCache;
        queries: GetMetadata, GetMetadataRaw, GetMetadataDefault, MetadataExists, GetMetadataByMid, SanitizeMeta,
            IsProtectedMeta, GetObjectSubtype, FilterDefaultMetadata);
}
