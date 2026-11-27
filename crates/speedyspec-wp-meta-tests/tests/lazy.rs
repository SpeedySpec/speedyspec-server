//! The metadata lazy loader against WordPress 7.2-RC2's `WP_Metadata_Lazyloader` (the `lazy-*` scenarios).

use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_php_serialize::PhpValue;
use speedyspec_wp_hooks::{IgnoreDeprecations, WpHookApi};
use speedyspec_wp_hooks_contracts::{Callback, HookName, HookValue, Key};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_meta::{
    LazyError, LazyLoader, MetaDeps, MetaKeyRegistry, MetadataApi, UnfilteredProtection,
};
use speedyspec_wp_meta_contracts::{
    Installing, Meta, MetaId, MetaKey, MetaType, Metadata, MetadataError, ObjectId, ObjectSubtypes,
    UpdateOutcome,
};
use speedyspec_wp_meta_memory::MemoryMetaRepositories;
use speedyspec_wp_object_cache::{ObjectCachePorts, WpObjectCache};

/// A `Metadata` that only records `update_cache`.
#[derive(Default)]
struct Spy {
    loads: RefCell<Vec<(String, Vec<u64>)>>,
}

impl Metadata for Spy {
    fn add(
        &self,
        _: &MetaType,
        _: ObjectId,
        _: &MetaKey,
        _: PhpValue,
        _: bool,
    ) -> Result<Option<MetaId>, MetadataError> {
        unimplemented!()
    }
    fn update(
        &self,
        _: &MetaType,
        _: ObjectId,
        _: &MetaKey,
        _: PhpValue,
        _: Option<PhpValue>,
    ) -> Result<UpdateOutcome, MetadataError> {
        unimplemented!()
    }
    fn delete(
        &self,
        _: &MetaType,
        _: ObjectId,
        _: &MetaKey,
        _: Option<PhpValue>,
        _: bool,
    ) -> Result<bool, MetadataError> {
        unimplemented!()
    }
    fn get(
        &self,
        _: &MetaType,
        _: ObjectId,
        _: Option<&MetaKey>,
        _: bool,
    ) -> Result<PhpValue, MetadataError> {
        unimplemented!()
    }
    fn get_raw(
        &self,
        _: &MetaType,
        _: ObjectId,
        _: Option<&MetaKey>,
        _: bool,
    ) -> Result<Option<PhpValue>, MetadataError> {
        unimplemented!()
    }
    fn get_default(
        &self,
        _: &MetaType,
        _: ObjectId,
        _: &MetaKey,
        _: bool,
    ) -> Result<PhpValue, MetadataError> {
        unimplemented!()
    }
    fn exists(&self, _: &MetaType, _: ObjectId, _: &MetaKey) -> Result<bool, MetadataError> {
        unimplemented!()
    }
    fn get_by_mid(&self, _: &MetaType, _: MetaId) -> Result<Option<Meta>, MetadataError> {
        unimplemented!()
    }
    fn update_by_mid(
        &self,
        _: &MetaType,
        _: MetaId,
        _: PhpValue,
        _: Option<&MetaKey>,
    ) -> Result<bool, MetadataError> {
        unimplemented!()
    }
    fn delete_by_mid(&self, _: &MetaType, _: MetaId) -> Result<bool, MetadataError> {
        unimplemented!()
    }
    fn update_cache(
        &self,
        t: &MetaType,
        objects: &[ObjectId],
    ) -> Result<Option<PhpValue>, MetadataError> {
        self.loads
            .borrow_mut()
            .push((t.name().to_owned(), objects.iter().map(|o| o.0).collect()));
        Ok(Some(PhpValue::Bool(true)))
    }
}

fn hooks() -> Rc<WpHookApi> {
    Rc::new(WpHookApi::new(
        Rc::new(MemoryHookStore::new()),
        Rc::new(IgnoreDeprecations),
    ))
}

fn ids(v: &[u64]) -> Vec<ObjectId> {
    v.iter().map(|i| ObjectId(*i)).collect()
}

fn term() -> MetaType {
    MetaType::new("term").unwrap()
}

fn null() -> PhpValue {
    PhpValue::Null
}

#[test]
fn queue_then_one_get_loads_all() {
    let h = hooks();
    let spy = Rc::new(Spy::default());
    let loader = Rc::new(LazyLoader::new(h.clone(), spy.clone()));
    loader.queue("term", &ids(&[1, 2, 3])).unwrap();
    assert_eq!(loader.pending("term"), ids(&[1, 2, 3]));

    // The filter callback PHP hooks: the first read for term 1 loads every queued id once.
    let returned = loader
        .load_for_object(&term(), null(), Some(ObjectId(1)))
        .unwrap();
    assert_eq!(returned, null());
    assert_eq!(*spy.loads.borrow(), [("term".to_owned(), vec![1, 2, 3])]);
    assert!(loader.pending("term").is_empty());

    // Reset: the next read has nothing to load.
    loader.load_for(&term(), null()).unwrap();
    assert_eq!(spy.loads.borrow().len(), 1);
}

#[test]
fn callback_adds_an_unqueued_object_and_skips_id_zero() {
    let spy = Rc::new(Spy::default());
    let loader = LazyLoader::new(hooks(), spy.clone());
    loader.queue("term", &ids(&[1, 2])).unwrap();
    loader
        .load_for_object(&term(), null(), Some(ObjectId(9)))
        .unwrap();
    loader.queue("term", &ids(&[2, 2, 1])).unwrap();
    loader
        .load_for_object(&term(), null(), Some(ObjectId(0)))
        .unwrap();
    assert_eq!(
        *spy.loads.borrow(),
        [
            ("term".to_owned(), vec![1, 2, 9]),
            ("term".to_owned(), vec![2, 1])
        ]
    );
    // Nothing queued: nothing loaded, even for an object id.
    loader
        .load_for_object(&term(), null(), Some(ObjectId(5)))
        .unwrap();
    assert_eq!(spy.loads.borrow().len(), 2);
}

#[test]
fn queue_fires_metadata_lazyloader_queued_objects() {
    let h = hooks();
    let seen: Rc<RefCell<Vec<Vec<HookValue>>>> = Rc::default();
    let log = seen.clone();
    h.add_filter(
        &HookName::from("metadata_lazyloader_queued_objects"),
        Callback::rust("record", move |args: &[HookValue]| {
            log.borrow_mut().push(args.to_vec());
            Ok(HookValue::Null)
        }),
        &HookValue::Int(10),
        3,
    )
    .unwrap();
    let loader = LazyLoader::new(h, Rc::new(Spy::default()));
    loader
        .queue_with("term", &ids(&[1, 2, 3]), HookValue::Str("loader".into()))
        .unwrap();
    // (object_ids, object_type, loader), the ids as a list, as the lazy-term scenario records.
    let list = HookValue::Array(
        (0..3)
            .map(|i| (Key::Int(i), HookValue::Int(i + 1)))
            .collect(),
    );
    assert_eq!(
        *seen.borrow(),
        [vec![
            list,
            HookValue::Str("term".into()),
            HookValue::Str("loader".into())
        ]]
    );
    // The action fires on every queue call, with that call's ids only.
    loader.queue("term", &ids(&[3])).unwrap();
    assert_eq!(seen.borrow().len(), 2);
    assert_eq!(seen.borrow()[1][2], HookValue::Null);
}

#[test]
fn unknown_type_is_an_error() {
    let loader = LazyLoader::new(hooks(), Rc::new(Spy::default()));
    for result in [loader.queue("post", &ids(&[1])), loader.reset("post")] {
        match result {
            Err(e @ LazyError::UnknownType(_)) => {
                assert_eq!(e.to_string(), "Invalid object type.");
                assert_eq!(e.code(), "invalid_object_type");
            }
            other => panic!("expected UnknownType, got {other:?}"),
        }
    }
    assert!(loader.pending("post").is_empty());
}

#[test]
fn settings_say_what_to_hook() {
    let term = LazyLoader::settings("term").unwrap();
    assert_eq!(term.filter, "get_term_metadata");
    assert_eq!(term.callback, "lazyload_meta_callback");
    assert_eq!(term.deprecated_method, Some("lazyload_term_meta"));
    assert_eq!(
        LazyLoader::settings("comment").unwrap().filter,
        "get_comment_metadata"
    );
    assert_eq!(
        LazyLoader::settings("blog").unwrap().filter,
        "get_blog_metadata"
    );
    assert!(LazyLoader::settings("post").is_none());
    assert_eq!(
        LazyLoader::types().collect::<Vec<_>>(),
        ["term", "comment", "blog"]
    );
}

struct NoSubtypes;
impl ObjectSubtypes for NoSubtypes {
    fn subtype_of(&self, _: &MetaType, _: ObjectId) -> String {
        String::new()
    }
}
struct NotInstalling;
impl Installing for NotInstalling {
    fn installing(&self) -> bool {
        false
    }
}

/// The whole path: a `get_term_metadata` callback, as PHP hooks, makes the first `get_metadata` fill the cache
/// for every queued term.
#[test]
fn a_real_get_fills_the_cache_for_the_queue() {
    let h = hooks();
    let cache = Rc::new(WpObjectCache::new(ObjectCachePorts::rust(), None));
    let api = MetadataApi::new(MetaDeps {
        hooks: h.clone(),
        cache: cache.clone(),
        repositories: Rc::new(MemoryMetaRepositories::new(&["term"])),
        keys: Rc::new(MetaKeyRegistry::new()),
        subtypes: Rc::new(NoSubtypes),
        protection: Rc::new(UnfilteredProtection),
        installing: Rc::new(NotInstalling),
    });
    for (term_id, value) in [(1, "a"), (2, "b")] {
        api.add_metadata(
            &term(),
            ObjectId(term_id),
            &MetaKey::from("k"),
            PhpValue::Str(value.as_bytes().to_vec()),
            false,
        )
        .unwrap();
    }
    cache.flush();
    let api_dyn: Rc<dyn Metadata> = api.clone();
    let loader = Rc::new(LazyLoader::new(h.clone(), api_dyn));
    let settings = LazyLoader::settings("term").unwrap();
    let l = loader.clone();
    h.add_filter(
        &HookName::from(settings.filter),
        Callback::rust(settings.callback, move |args: &[HookValue]| {
            let object = match args.get(1) {
                Some(HookValue::Int(i)) => Some(ObjectId(*i as u64)),
                _ => None,
            };
            l.load_for_object(&term(), PhpValue::Null, object)
                .map_err(|e| speedyspec_wp_hooks_contracts::HookError::message(e.to_string()))?;
            Ok(args.first().cloned().unwrap_or(HookValue::Null))
        }),
        &HookValue::Int(10),
        5,
    )
    .unwrap();
    loader.queue("term", &ids(&[1, 2, 3])).unwrap();
    let group = HookValue::Str(term().cache_group());
    assert!(cache.peek(&HookValue::Int(2), &group).is_none());
    let got = api
        .get_metadata(&term(), ObjectId(1), Some(&MetaKey::from("k")), true)
        .unwrap();
    assert_eq!(got, PhpValue::Str(b"a".to_vec()));
    for id in [1, 2, 3] {
        assert!(
            cache.peek(&HookValue::Int(id), &group).is_some(),
            "term {id}"
        );
    }
    assert!(loader.pending("term").is_empty());
}

/// A failing `metadata_lazyloader_queued_objects` callback reaches the caller with its payload intact, so a bridge
/// can rethrow the PHP exception.
#[test]
fn hook_error_keeps_its_payload() {
    let h = hooks();
    let payload: Rc<dyn std::any::Any> = Rc::new("the exception".to_owned());
    let thrown = payload.clone();
    h.add_filter(
        &HookName::from("metadata_lazyloader_queued_objects"),
        Callback::rust("throws", move |_: &[HookValue]| {
            Err(speedyspec_wp_hooks_contracts::HookError {
                message: "thrown".into(),
                payload: Some(thrown.clone()),
            })
        }),
        &HookValue::Int(10),
        3,
    )
    .unwrap();
    let loader = LazyLoader::new(h, Rc::new(Spy::default()));
    let err = loader.queue("term", &ids(&[1])).unwrap_err();
    assert!(matches!(err, LazyError::Hook(_)));
    assert!(Rc::ptr_eq(err.payload().expect("a payload"), &payload));
    assert_eq!(err.hook_error().map(|e| e.message.as_str()), Some("thrown"));
    // The queue still holds the id, as WordPress's does after its action throws.
    assert_eq!(loader.pending("term"), ids(&[1]));
    // An unknown type has none.
    assert!(loader.reset("post").unwrap_err().payload().is_none());
}
