//! The metadata use cases over `MemoryMetaRepositories`, `WpHookApi` (memory store), the Rust object cache,
//! `MetaKeyRegistry` and fixed-answer ports, following WordPress 7.2-RC2's `meta.php`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use speedyspec_container::Container;
use speedyspec_cqrs::{Command, CommandBus, Query, QueryBus};
use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_hooks::{IgnoreDeprecations, WpHookApi};
use speedyspec_wp_hooks_contracts::{Callback, HookError, HookName, HookValue, Key};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_meta::{
    AddMetadata, AddOutcome, BlogSubtypes, ByMidOutcome, DeleteMetadata, DeleteMetadataByMid,
    FilterDefaultMetadata, FilteredProtection, GetMetadata, GetMetadataByMid, GetMetadataDefault,
    GetMetadataRaw, GetObjectSubtype, IsProtectedMeta, MetaDeps, MetaKeyRegistry, MetadataApi,
    MetadataExists, SanitizeMeta, Sites, SubtypesByType, UnfilteredProtection, UpdateMetaCache,
    UpdateMetadata, UpdateMetadataByMid, object_subtype, read_hooks, register_meta_handlers,
};
use speedyspec_wp_meta_contracts::{
    Installing, Meta, MetaError, MetaId, MetaKey, MetaRepositories, MetaRepository, MetaType,
    Metadata, MetadataError, ObjectId, ObjectSubtypes, ProtectedMeta, RegisteredMetaKey,
    SerializedValue, UpdateOutcome,
};
use speedyspec_wp_meta_memory::MemoryMetaRepositories;
use speedyspec_wp_object_cache::{ObjectCachePorts, WpObjectCache};

// ---- fixed-answer ports and a counting repository ----

/// Every repository call by name, and whether `add` should fail as `$wpdb->insert` does on invalid bytes.
#[derive(Default)]
struct Calls {
    log: RefCell<Vec<&'static str>>,
    fail_add: Cell<bool>,
}

impl Calls {
    fn count(&self, name: &str) -> usize {
        self.log.borrow().iter().filter(|c| **c == name).count()
    }
    fn total(&self) -> usize {
        self.log.borrow().len()
    }
    fn reset(&self) {
        self.log.borrow_mut().clear();
    }
}

struct Counting {
    inner: Rc<dyn MetaRepository>,
    calls: Rc<Calls>,
}

impl Counting {
    fn note(&self, name: &'static str) {
        self.calls.log.borrow_mut().push(name);
    }
}

impl MetaRepository for Counting {
    fn engine(&self) -> String {
        self.inner.engine()
    }
    fn meta_type(&self) -> &MetaType {
        self.inner.meta_type()
    }
    fn count_for(&self, object: ObjectId, key: &MetaKey) -> Result<u64, MetaError> {
        self.note("count_for");
        self.inner.count_for(object, key)
    }
    fn add(
        &self,
        object: ObjectId,
        key: &MetaKey,
        value: Option<&SerializedValue>,
    ) -> Result<Option<MetaId>, MetaError> {
        self.note("add");
        if self.calls.fail_add.get() {
            return Ok(None);
        }
        self.inner.add(object, key, value)
    }
    fn ids_for(&self, object: ObjectId, key: &MetaKey) -> Result<Vec<MetaId>, MetaError> {
        self.note("ids_for");
        self.inner.ids_for(object, key)
    }
    fn update_by_key(
        &self,
        object: ObjectId,
        key: &MetaKey,
        value: Option<&SerializedValue>,
        previous: Option<&SerializedValue>,
    ) -> Result<u64, MetaError> {
        self.note("update_by_key");
        self.inner.update_by_key(object, key, value, previous)
    }
    fn ids_matching(
        &self,
        key: &MetaKey,
        object: Option<ObjectId>,
        value: Option<&SerializedValue>,
    ) -> Result<Vec<MetaId>, MetaError> {
        self.note("ids_matching");
        self.inner.ids_matching(key, object, value)
    }
    fn objects_with(
        &self,
        key: &MetaKey,
        value: Option<&SerializedValue>,
    ) -> Result<Vec<ObjectId>, MetaError> {
        self.note("objects_with");
        self.inner.objects_with(key, value)
    }
    fn delete_ids(&self, ids: &[MetaId]) -> Result<u64, MetaError> {
        self.note("delete_ids");
        self.inner.delete_ids(ids)
    }
    fn by_mid(&self, mid: MetaId) -> Result<Option<Meta>, MetaError> {
        self.note("by_mid");
        self.inner.by_mid(mid)
    }
    fn update_by_mid(
        &self,
        mid: MetaId,
        key: &MetaKey,
        value: Option<&SerializedValue>,
    ) -> Result<u64, MetaError> {
        self.note("update_by_mid");
        self.inner.update_by_mid(mid, key, value)
    }
    fn delete_by_mid(&self, mid: MetaId) -> Result<u64, MetaError> {
        self.note("delete_by_mid");
        self.inner.delete_by_mid(mid)
    }
    fn for_objects(&self, objects: &[ObjectId]) -> Result<Vec<Meta>, MetaError> {
        self.note("for_objects");
        self.inner.for_objects(objects)
    }
}

struct CountingRepositories {
    inner: MemoryMetaRepositories,
    calls: Rc<Calls>,
}

impl MetaRepositories for CountingRepositories {
    fn for_type(&self, meta_type: &MetaType) -> Option<Rc<dyn MetaRepository>> {
        let inner = self.inner.for_type(meta_type)?;
        Some(Rc::new(Counting {
            inner,
            calls: self.calls.clone(),
        }))
    }
}

/// `get_object_subtype()`'s switch: posts 1-9 are posts, 10 and up pages; comments are comments.
struct Subtypes {
    asked: Cell<usize>,
}

impl ObjectSubtypes for Subtypes {
    fn subtype_of(&self, meta_type: &MetaType, object: ObjectId) -> String {
        self.asked.set(self.asked.get() + 1);
        match (meta_type.name(), object.0) {
            ("post", 10..) => "page".into(),
            ("post", _) => "post".into(),
            ("comment", _) => "comment".into(),
            _ => String::new(),
        }
    }
}

#[derive(Default)]
struct InstallingFlag(Cell<bool>);

impl Installing for InstallingFlag {
    fn installing(&self) -> bool {
        self.0.get()
    }
}

type Log = Rc<RefCell<Vec<(String, Vec<HookValue>)>>>;

struct Fixture {
    api: Rc<MetadataApi>,
    hooks: Rc<WpHookApi>,
    cache: Rc<WpObjectCache>,
    calls: Rc<Calls>,
    subtypes: Rc<Subtypes>,
    installing: Rc<InstallingFlag>,
    log: Log,
}

fn fixture_with(registry: MetaKeyRegistry) -> Fixture {
    let hooks = Rc::new(WpHookApi::new(
        Rc::new(MemoryHookStore::new()),
        Rc::new(IgnoreDeprecations),
    ));
    let cache = Rc::new(WpObjectCache::new(ObjectCachePorts::rust(), None));
    let calls = Rc::new(Calls::default());
    let subtypes = Rc::new(Subtypes {
        asked: Cell::new(0),
    });
    let installing = Rc::new(InstallingFlag::default());
    let api = MetadataApi::new(MetaDeps {
        hooks: hooks.clone(),
        cache: cache.clone(),
        repositories: Rc::new(CountingRepositories {
            inner: MemoryMetaRepositories::new(&["post", "comment", "user", "term"]),
            calls: calls.clone(),
        }),
        keys: Rc::new(registry),
        subtypes: subtypes.clone(),
        protection: Rc::new(UnfilteredProtection),
        installing: installing.clone(),
    });
    Fixture {
        api,
        hooks,
        cache,
        calls,
        subtypes,
        installing,
        log: Rc::new(RefCell::new(Vec::new())),
    }
}

fn fixture() -> Fixture {
    fixture_with(MetaKeyRegistry::new())
}

fn post() -> MetaType {
    MetaType::new("post").unwrap()
}

fn comment() -> MetaType {
    MetaType::new("comment").unwrap()
}

fn k(key: &str) -> MetaKey {
    MetaKey::from(key)
}

fn s(v: &str) -> PhpValue {
    PhpValue::Str(v.as_bytes().to_vec())
}

fn hs(v: &str) -> HookValue {
    HookValue::Str(v.into())
}

fn list(values: &[PhpValue]) -> PhpValue {
    PhpValue::Array(
        values
            .iter()
            .enumerate()
            .map(|(i, v)| (PhpKey::Int(i as i64), v.clone()))
            .collect(),
    )
}

/// Logs each run of `hook` with its arguments; the callback returns its first argument.
fn record(f: &Fixture, hook: &str) {
    let log = f.log.clone();
    let name = hook.to_string();
    f.hooks
        .add_filter(
            &HookName::from(hook),
            Callback::rust(&format!("record_{hook}"), move |args: &[HookValue]| {
                log.borrow_mut().push((name.clone(), args.to_vec()));
                Ok(args.first().cloned().unwrap_or(HookValue::Null))
            }),
            &HookValue::Int(10),
            99,
        )
        .unwrap();
}

/// A callback at priority 20 that answers `value` whatever it gets.
fn answer(f: &Fixture, hook: &str, value: HookValue) {
    f.hooks
        .add_filter(
            &HookName::from(hook),
            Callback::rust(&format!("answer_{hook}"), move |_: &[HookValue]| {
                Ok(value.clone())
            }),
            &HookValue::Int(20),
            99,
        )
        .unwrap();
}

fn fired(f: &Fixture) -> Vec<String> {
    f.log.borrow().iter().map(|(h, _)| h.clone()).collect()
}

fn args_of(f: &Fixture, hook: &str) -> Vec<HookValue> {
    f.log
        .borrow()
        .iter()
        .find(|(h, _)| h == hook)
        .map(|(_, a)| a.clone())
        .unwrap_or_else(|| panic!("{hook} did not fire"))
}

fn cached(f: &Fixture, meta_type: &MetaType, object: u64) -> Option<HookValue> {
    f.cache.peek(
        &HookValue::Int(object as i64),
        &HookValue::Str(meta_type.cache_group()),
    )
}

fn add(f: &Fixture, meta_type: &MetaType, object: u64, key: &str, value: PhpValue) -> MetaId {
    match f
        .api
        .add_metadata(meta_type, ObjectId(object), &k(key), value, false)
        .unwrap()
    {
        AddOutcome::Added(mid) => mid,
        other => panic!("add failed: {other:?}"),
    }
}

// ---- the plan's tests ----

#[test]
fn add_fires_add_then_added_with_mid() {
    let f = fixture();
    for hook in ["add_post_metadata", "add_post_meta", "added_post_meta"] {
        record(&f, hook);
    }
    let mid = add(&f, &post(), 1, "k", s("v"));
    assert_eq!(mid, MetaId(1));
    assert_eq!(
        fired(&f),
        ["add_post_metadata", "add_post_meta", "added_post_meta"]
    );
    assert_eq!(
        args_of(&f, "add_post_metadata"),
        [
            HookValue::Null,
            HookValue::Int(1),
            hs("k"),
            hs("v"),
            HookValue::Bool(false)
        ]
    );
    assert_eq!(
        args_of(&f, "add_post_meta"),
        [HookValue::Int(1), hs("k"), hs("v")]
    );
    assert_eq!(
        args_of(&f, "added_post_meta"),
        [HookValue::Int(1), HookValue::Int(1), hs("k"), hs("v")]
    );
    // The Metadata contract answers the id.
    let second = Metadata::add(&*f.api, &post(), ObjectId(1), &k("k"), s("w"), false).unwrap();
    assert_eq!(second, Some(MetaId(2)));
    // A unique key that exists: false, after one count query.
    f.calls.reset();
    assert_eq!(
        Metadata::add(&*f.api, &post(), ObjectId(1), &k("k"), s("x"), true).unwrap(),
        None
    );
    assert_eq!(*f.calls.log.borrow(), ["count_for"]);
}

#[test]
fn add_reports_false_when_insert_fails() {
    let f = fixture();
    record(&f, "add_post_meta");
    record(&f, "added_post_meta");
    f.calls.fail_add.set(true);
    let got = f
        .api
        .add_metadata(&post(), ObjectId(1), &k("k"), s("v"), false)
        .unwrap();
    assert_eq!(got, AddOutcome::Failed);
    assert_eq!(
        Metadata::add(&*f.api, &post(), ObjectId(1), &k("k"), s("v"), false).unwrap(),
        None
    );
    // add_post_meta fired before the insert; added_post_meta never did.
    assert_eq!(fired(&f), ["add_post_meta", "add_post_meta"]);
}

#[test]
fn update_same_value_is_unchanged() {
    let f = fixture();
    add(&f, &post(), 1, "k", s("v"));
    record(&f, "update_post_meta");
    f.calls.reset();
    let got = f
        .api
        .update_metadata(&post(), ObjectId(1), &k("k"), s("v"), None)
        .unwrap();
    assert_eq!(got, UpdateOutcome::Unchanged);
    // Only the cache load ran (get_metadata_raw); no id lookup, no write.
    assert_eq!(*f.calls.log.borrow(), ["for_objects"]);
    assert!(fired(&f).is_empty());
    // A changed value writes and answers true.
    record(&f, "updated_post_meta");
    let got = f
        .api
        .update_metadata(&post(), ObjectId(1), &k("k"), s("w"), None)
        .unwrap();
    assert_eq!(got, UpdateOutcome::Updated);
    assert_eq!(fired(&f), ["update_post_meta", "updated_post_meta"]);
    // update_{type}_meta gets get_col's string id.
    assert_eq!(args_of(&f, "update_post_meta")[0], hs("1"));
    assert!(cached(&f, &post(), 1).is_none());
}

#[test]
fn update_missing_adds() {
    let f = fixture();
    record(&f, "add_post_meta");
    let got = f
        .api
        .update_metadata(&post(), ObjectId(1), &k("k"), s("v"), None)
        .unwrap();
    assert_eq!(got, UpdateOutcome::Added(MetaId(1)));
    assert_eq!(fired(&f), ["add_post_meta"]);
    assert_eq!(
        f.api
            .get_metadata(&post(), ObjectId(1), Some(&k("k")), true)
            .unwrap(),
        s("v")
    );
}

#[test]
fn delete_all_clears_every_object_cache() {
    let f = fixture();
    add(&f, &post(), 1, "k", s("a"));
    add(&f, &post(), 2, "k", s("b"));
    add(&f, &post(), 3, "keep", s("c"));
    for id in [1, 2, 3] {
        f.api
            .get_metadata(&post(), ObjectId(id), None, false)
            .unwrap();
        assert!(cached(&f, &post(), id).is_some());
    }
    record(&f, "delete_post_meta");
    let deleted = f
        .api
        .delete_metadata(&post(), ObjectId(0), &k("k"), None, true)
        .unwrap();
    assert!(deleted);
    assert!(cached(&f, &post(), 1).is_none());
    assert!(cached(&f, &post(), 2).is_none());
    assert!(cached(&f, &post(), 3).is_some());
    assert_eq!(
        args_of(&f, "delete_post_meta")[0],
        HookValue::Array(vec![(Key::Int(0), hs("1")), (Key::Int(1), hs("2"))])
    );
    assert_eq!(
        f.api
            .get_metadata(&post(), ObjectId(1), Some(&k("k")), false)
            .unwrap(),
        PhpValue::Array(Vec::new())
    );
}

#[test]
fn legacy_postmeta_actions_only_for_posts() {
    let f = fixture();
    let legacy = [
        "update_postmeta",
        "updated_postmeta",
        "delete_postmeta",
        "deleted_postmeta",
        "update_commentmeta",
        "updated_commentmeta",
        "delete_commentmeta",
        "deleted_commentmeta",
    ];
    for hook in legacy {
        record(&f, hook);
    }
    add(&f, &post(), 1, "k", s("a"));
    add(&f, &comment(), 1, "k", s("a"));
    f.api
        .update_metadata(&post(), ObjectId(1), &k("k"), list(&[s("arr")]), None)
        .unwrap();
    f.api
        .update_metadata(&comment(), ObjectId(1), &k("k"), s("b"), None)
        .unwrap();
    // The old-style post actions get the serialized value.
    assert_eq!(
        args_of(&f, "update_postmeta"),
        [
            hs("1"),
            HookValue::Int(1),
            hs("k"),
            hs("a:1:{i:0;s:3:\"arr\";}")
        ]
    );
    f.api
        .delete_metadata(&post(), ObjectId(1), &k("k"), None, false)
        .unwrap();
    f.api
        .delete_metadata(&comment(), ObjectId(1), &k("k"), None, false)
        .unwrap();
    assert_eq!(
        fired(&f),
        [
            "update_postmeta",
            "updated_postmeta",
            "delete_postmeta",
            "deleted_postmeta"
        ]
    );
    // By mid, comments have their own old-style delete actions too.
    f.log.borrow_mut().clear();
    let post_mid = add(&f, &post(), 1, "m", s("a"));
    let comment_mid = add(&f, &comment(), 1, "m", s("a"));
    assert!(f.api.delete_metadata_by_mid(&post(), post_mid).unwrap());
    assert!(
        f.api
            .delete_metadata_by_mid(&comment(), comment_mid)
            .unwrap()
    );
    assert_eq!(
        fired(&f),
        [
            "delete_postmeta",
            "deleted_postmeta",
            "delete_commentmeta",
            "deleted_commentmeta"
        ]
    );
    // delete_metadata_by_mid passes the int id.
    assert_eq!(
        args_of(&f, "delete_postmeta"),
        [HookValue::Int(post_mid.0 as i64)]
    );
}

#[test]
fn get_miss_loads_every_key_once() {
    let f = fixture();
    add(&f, &post(), 1, "k1", s("a"));
    add(&f, &post(), 1, "k2", s("b"));
    f.calls.reset();
    assert_eq!(
        f.api
            .get_metadata(&post(), ObjectId(1), Some(&k("k1")), true)
            .unwrap(),
        s("a")
    );
    assert_eq!(f.calls.count("for_objects"), 1);
    assert_eq!(
        f.api
            .get_metadata(&post(), ObjectId(1), Some(&k("k2")), true)
            .unwrap(),
        s("b")
    );
    assert_eq!(f.calls.total(), 1);
    // The entry is PHP-shaped: key => list of stored strings.
    assert_eq!(
        cached(&f, &post(), 1),
        Some(HookValue::Array(vec![
            (
                Key::Str("k1".into()),
                HookValue::Array(vec![(Key::Int(0), hs("a"))])
            ),
            (
                Key::Str("k2".into()),
                HookValue::Array(vec![(Key::Int(0), hs("b"))])
            ),
        ]))
    );
}

#[test]
fn reads_php_shaped_cache_entry() {
    let f = fixture();
    let entry = HookValue::Array(vec![(
        Key::Str("k".into()),
        HookValue::Array(vec![(Key::Int(0), hs("a:1:{i:0;s:1:\"x\";}"))]),
    )]);
    f.cache
        .set(
            &HookValue::Int(1),
            entry,
            &HookValue::Str("post_meta".into()),
            0,
        )
        .unwrap();
    let got = f
        .api
        .get_metadata(&post(), ObjectId(1), Some(&k("k")), true)
        .unwrap();
    assert_eq!(got, list(&[s("x")]));
    assert_eq!(f.calls.total(), 0);
    // Several values come back unserialized one by one; the array itself is not unserialized.
    let all = f
        .api
        .get_metadata(&post(), ObjectId(1), Some(&k("k")), false)
        .unwrap();
    assert_eq!(all, list(&[list(&[s("x")])]));
    assert_eq!(f.calls.total(), 0);
}

#[test]
fn default_skipped_while_installing() {
    let mut registry = MetaKeyRegistry::new();
    registry.insert(RegisteredMetaKey {
        object_type: "post".into(),
        subtype: String::new(),
        key: k("dk"),
        args: vec![(PhpKey::Str(b"default".to_vec()), s("dflt"))],
    });
    let f = fixture_with(registry);
    // register_meta() adds filter_default_metadata to default_post_metadata; here a Rust callback stands for it.
    let api: Weak<MetadataApi> = Rc::downgrade(&f.api);
    f.hooks
        .add_filter(
            &HookName::from("default_post_metadata"),
            Callback::rust("filter_default_metadata", move |args: &[HookValue]| {
                let api = api.upgrade().expect("the api outlives its hooks");
                let value = speedyspec_php_serialize::from_hook_value(&args[0]).unwrap();
                let HookValue::Int(object) = args[1] else {
                    panic!("an int object id")
                };
                let key = match &args[2] {
                    HookValue::Str(s) => MetaKey::from(s.as_str()),
                    other => panic!("a key, not {other:?}"),
                };
                let HookValue::Bool(single) = args[3] else {
                    panic!("a bool single")
                };
                let out = api
                    .filter_default_metadata(value, ObjectId(object as u64), &key, single, &post())
                    .map_err(|e| HookError::message(e.to_string()))?;
                Ok(speedyspec_php_serialize::to_hook_value(&out))
            }),
            &HookValue::Int(10),
            5,
        )
        .unwrap();
    let get = |single| {
        f.api
            .get_metadata(&post(), ObjectId(1), Some(&k("dk")), single)
            .unwrap()
    };
    assert_eq!(get(true), s("dflt"));
    assert_eq!(get(false), list(&[s("dflt")]));
    f.installing.0.set(true);
    assert_eq!(get(true), s(""));
    assert_eq!(get(false), PhpValue::Array(Vec::new()));
    f.installing.0.set(false);
    assert_eq!(get(true), s("dflt"));
    // A "" default needs no subtype lookup.
    assert_eq!(f.subtypes.asked.get(), 0);
}

#[test]
fn read_hooks_lists_short_circuit_and_default() {
    let names: Vec<HookValue> = read_hooks(&post())
        .iter()
        .map(|n| n.value().clone())
        .collect();
    assert_eq!(
        names,
        [hs("get_post_metadata"), hs("default_post_metadata")]
    );
    let names: Vec<HookValue> = read_hooks(&MetaType::new("thing").unwrap())
        .iter()
        .map(|n| n.value().clone())
        .collect();
    assert_eq!(
        names,
        [hs("get_thing_metadata"), hs("default_thing_metadata")]
    );
}

#[test]
fn protected_filter_overrides_rule() {
    let f = fixture();
    let protection = FilteredProtection::new(f.hooks.clone(), Rc::new(UnfilteredProtection));
    assert!(protection.is_protected(&k("_secret"), "post"));
    assert!(!protection.is_protected(&k("color"), "post"));
    record(&f, "is_protected_meta");
    answer(&f, "is_protected_meta", HookValue::Bool(false));
    assert!(!protection.is_protected(&k("_secret"), "post"));
    assert_eq!(
        args_of(&f, "is_protected_meta"),
        [HookValue::Bool(true), hs("_secret"), hs("post")]
    );
    let f = fixture();
    let protection = FilteredProtection::new(f.hooks.clone(), Rc::new(UnfilteredProtection));
    answer(&f, "is_protected_meta", HookValue::Int(1));
    assert!(protection.is_protected(&k("color"), "post"));
}

#[test]
fn hook_error_stops_before_write() {
    let f = fixture();
    f.hooks
        .add_filter(
            &HookName::from("add_post_metadata"),
            Callback::rust("boom", |_: &[HookValue]| Err(HookError::message("boom"))),
            &HookValue::Int(10),
            5,
        )
        .unwrap();
    let got = f
        .api
        .add_metadata(&post(), ObjectId(1), &k("k"), s("v"), false);
    match got {
        Err(MetadataError::Hook(e)) => assert_eq!(e.message, "boom"),
        other => panic!("expected the hook's error, got {other:?}"),
    }
    assert_eq!(f.calls.total(), 0);
}

// ---- more of meta.php, as Task 3 recorded it ----

#[test]
fn add_unslashes_sanitizes_and_serializes() {
    let f = fixture();
    record(&f, "sanitize_post_meta_k\\'");
    record(&f, "sanitize_post_meta_k'_for_page");
    record(&f, "get_object_subtype_post");
    add(&f, &post(), 10, "k\\'", list(&[s("a\\'b")]));
    // The key and value lose their slashes before sanitize_meta(); the page subtype has a callback, so only it runs.
    assert_eq!(
        fired(&f),
        ["get_object_subtype_post", "sanitize_post_meta_k'_for_page"]
    );
    assert_eq!(
        args_of(&f, "get_object_subtype_post"),
        [hs("page"), HookValue::Int(10)]
    );
    let got = f
        .api
        .get_metadata(&post(), ObjectId(10), Some(&k("k'")), true)
        .unwrap();
    assert_eq!(got, list(&[s("a'b")]));
}

#[test]
fn update_with_prev_value_fires_for_every_id() {
    let f = fixture();
    add(&f, &post(), 1, "k", s("a"));
    add(&f, &post(), 1, "k", s("b"));
    record(&f, "update_post_meta");
    record(&f, "updated_post_meta");
    let got = f
        .api
        .update_metadata(&post(), ObjectId(1), &k("k"), s("c"), Some(s("b")))
        .unwrap();
    assert_eq!(got, UpdateOutcome::Updated);
    assert_eq!(
        fired(&f),
        [
            "update_post_meta",
            "update_post_meta",
            "updated_post_meta",
            "updated_post_meta"
        ]
    );
    assert_eq!(
        f.api
            .get_metadata(&post(), ObjectId(1), Some(&k("k")), false)
            .unwrap(),
        list(&[s("a"), s("c")])
    );
    // No row has the previous value: update_* fires for each id, and the call is false.
    f.log.borrow_mut().clear();
    let got = f
        .api
        .update_metadata(&post(), ObjectId(1), &k("k"), s("d"), Some(s("zzz")))
        .unwrap();
    assert_eq!(got, UpdateOutcome::Failed);
    assert_eq!(fired(&f), ["update_post_meta", "update_post_meta"]);
}

#[test]
fn update_int_for_stored_string_writes_but_changes_nothing() {
    let f = fixture();
    add(&f, &post(), 1, "k", s("5"));
    record(&f, "update_post_meta");
    record(&f, "updated_post_meta");
    let got = f
        .api
        .update_metadata(&post(), ObjectId(1), &k("k"), PhpValue::Int(5), None)
        .unwrap();
    // "5" !== 5, so it writes; no row changes, so it is false with no updated_* action.
    assert_eq!(got, UpdateOutcome::Failed);
    assert_eq!(fired(&f), ["update_post_meta"]);
}

#[test]
fn cache_miss_repairs_a_non_array_entry() {
    let f = fixture();
    add(&f, &post(), 1, "k", s("stored"));
    let group = HookValue::Str("post_meta".into());
    for bad in [hs("garbage"), HookValue::Int(5), HookValue::Bool(true)] {
        f.cache.set(&HookValue::Int(1), bad, &group, 0).unwrap();
        f.calls.reset();
        let got = f
            .api
            .get_metadata(&post(), ObjectId(1), Some(&k("k")), true)
            .unwrap();
        assert_eq!(got, s("stored"));
        assert_eq!(f.calls.count("for_objects"), 1);
        assert_eq!(
            cached(&f, &post(), 1),
            Some(HookValue::Array(vec![(
                Key::Str("k".into()),
                HookValue::Array(vec![(Key::Int(0), hs("stored"))])
            )]))
        );
    }
    // metadata_exists() reloads the same way.
    f.cache
        .set(&HookValue::Int(1), hs("garbage"), &group, 0)
        .unwrap();
    assert!(
        f.api
            .metadata_exists(&post(), ObjectId(1), &k("k"))
            .unwrap()
    );
}

#[test]
fn cache_miss_keeps_an_empty_array() {
    let f = fixture();
    add(&f, &post(), 1, "k", s("stored"));
    let group = HookValue::Str("post_meta".into());
    f.cache
        .set(&HookValue::Int(1), HookValue::Array(Vec::new()), &group, 0)
        .unwrap();
    f.calls.reset();
    let got = f
        .api
        .get_metadata(&post(), ObjectId(1), Some(&k("k")), true)
        .unwrap();
    // `! $meta_cache` reloads, but wp_cache_get_multiple finds array(), which counts as cached.
    assert_eq!(got, s(""));
    assert_eq!(f.calls.total(), 0);
    assert_eq!(cached(&f, &post(), 1), Some(HookValue::Array(Vec::new())));
}

#[test]
fn update_meta_cache_queries_only_the_uncached() {
    let f = fixture();
    add(&f, &post(), 1, "k", s("a"));
    add(&f, &post(), 2, "k", s("b"));
    f.api
        .get_metadata(&post(), ObjectId(1), None, false)
        .unwrap();
    let group = HookValue::Str("post_meta".into());
    f.cache
        .set(&HookValue::Int(2), hs("garbage"), &group, 0)
        .unwrap();
    f.calls.reset();
    let got = f
        .api
        .update_meta_cache(&post(), &[ObjectId(1), ObjectId(2), ObjectId(3)])
        .unwrap()
        .unwrap();
    assert_eq!(f.calls.count("for_objects"), 1);
    let entry = |v: &str| PhpValue::Array(vec![(PhpKey::Str(b"k".to_vec()), list(&[s(v)]))]);
    assert_eq!(
        got,
        PhpValue::Array(vec![
            (PhpKey::Int(1), entry("a")),
            (PhpKey::Int(2), entry("b")),
            (PhpKey::Int(3), PhpValue::Array(Vec::new())),
        ])
    );
    assert_eq!(cached(&f, &post(), 3), Some(HookValue::Array(Vec::new())));
    // No ids, or an unknown type: false.
    assert_eq!(f.api.update_meta_cache(&post(), &[]).unwrap(), None);
    let nope = MetaType::new("nope").unwrap();
    assert_eq!(
        f.api.update_meta_cache(&nope, &[ObjectId(1)]).unwrap(),
        None
    );
    // A short-circuit is read for truth.
    answer(&f, "update_post_metadata_cache", HookValue::Int(1));
    assert_eq!(
        f.api.update_meta_cache(&post(), &[ObjectId(9)]).unwrap(),
        Some(PhpValue::Bool(true))
    );
}

#[test]
fn get_short_circuit_single_reads_index_zero() {
    let f = fixture();
    let assoc = HookValue::Array(vec![
        (Key::Str("a".into()), hs("x")),
        (Key::Int(0), hs("zero")),
    ]);
    answer(&f, "get_post_metadata", assoc);
    let get = |single| {
        f.api
            .get_metadata(&post(), ObjectId(1), Some(&k("k")), single)
            .unwrap()
    };
    assert_eq!(get(true), s("zero"));
    let f = fixture();
    answer(&f, "get_post_metadata", HookValue::Array(Vec::new()));
    // `array()[0]` is null (with PHP's warning), so the default '' follows.
    assert_eq!(
        f.api
            .get_metadata(&post(), ObjectId(1), Some(&k("k")), true)
            .unwrap(),
        s("")
    );
    assert_eq!(f.calls.total(), 0);
}

#[test]
fn invalid_arguments_answer_false() {
    let f = fixture();
    assert_eq!(
        f.api
            .add_metadata(&post(), ObjectId(0), &k("k"), s("v"), false)
            .unwrap(),
        AddOutcome::Failed
    );
    assert_eq!(
        f.api
            .add_metadata(&post(), ObjectId(1), &k("0"), s("v"), false)
            .unwrap(),
        AddOutcome::Failed
    );
    let nope = MetaType::new("nope").unwrap();
    assert_eq!(
        f.api
            .add_metadata(&nope, ObjectId(1), &k("k"), s("v"), false)
            .unwrap(),
        AddOutcome::Failed
    );
    assert_eq!(
        f.api
            .get_metadata_raw(&post(), ObjectId(0), Some(&k("k")), true)
            .unwrap(),
        Some(PhpValue::Bool(false))
    );
    assert!(
        !f.api
            .delete_metadata(&post(), ObjectId(0), &k("k"), None, false)
            .unwrap()
    );
    assert_eq!(
        f.api.get_metadata_by_mid(&post(), MetaId(0)).unwrap(),
        ByMidOutcome::NotFound
    );
    // No subtype lookup happened for any of them.
    assert_eq!(f.subtypes.asked.get(), 0);
}

#[test]
fn by_mid_round_trip() {
    let f = fixture();
    let mid = add(&f, &post(), 1, "k", s("a"));
    for hook in [
        "update_post_meta",
        "update_postmeta",
        "updated_post_meta",
        "delete_post_meta",
    ] {
        record(&f, hook);
    }
    let ByMidOutcome::Found(meta) = f.api.get_metadata_by_mid(&post(), mid).unwrap() else {
        panic!("a row");
    };
    assert_eq!(meta.key, k("k"));
    assert!(
        f.api
            .update_metadata_by_mid(&post(), mid, list(&[s("c")]), Some(&k("renamed")))
            .unwrap()
    );
    // update_metadata_by_mid passes the int id and the row's object id as its column string.
    assert_eq!(
        args_of(&f, "update_post_meta"),
        [
            HookValue::Int(1),
            hs("1"),
            hs("renamed"),
            HookValue::Array(vec![(Key::Int(0), hs("c"))])
        ]
    );
    assert_eq!(
        args_of(&f, "update_postmeta")[3],
        hs("a:1:{i:0;s:1:\"c\";}")
    );
    assert!(f.api.delete_metadata_by_mid(&post(), mid).unwrap());
    assert_eq!(
        args_of(&f, "delete_post_meta"),
        [
            HookValue::Array(vec![(Key::Int(0), HookValue::Int(1))]),
            HookValue::Int(1),
            hs("renamed"),
            HookValue::Array(vec![(Key::Int(0), hs("c"))])
        ]
    );
    assert_eq!(
        f.api.get_metadata_by_mid(&post(), mid).unwrap(),
        ByMidOutcome::NotFound
    );
    // A short-circuit answer comes back as it is.
    answer(&f, "get_post_metadata_by_mid", hs("short"));
    assert_eq!(
        f.api.get_metadata_by_mid(&post(), mid).unwrap(),
        ByMidOutcome::Filtered(s("short"))
    );
}

#[test]
fn add_short_circuit_is_returned_as_is() {
    let f = fixture();
    answer(&f, "add_post_metadata", HookValue::Int(0));
    let got = f
        .api
        .add_metadata(&post(), ObjectId(1), &k("k"), s("v"), false)
        .unwrap();
    assert_eq!(got, AddOutcome::Filtered(PhpValue::Int(0)));
    assert_eq!(got.to_php(), PhpValue::Int(0));
    assert_eq!(f.calls.total(), 0);
}

#[test]
fn object_subtype_runs_its_filter() {
    let f = fixture();
    answer(&f, "get_object_subtype_post", hs("book"));
    let got = object_subtype(&*f.hooks, &*f.subtypes, &post(), ObjectId(1)).unwrap();
    assert_eq!(got, "book");
}

struct FixedSites {
    multisite: bool,
    looked_up: Cell<usize>,
}

impl Sites for FixedSites {
    fn is_multisite(&self) -> bool {
        self.multisite
    }
    fn site_exists(&self, id: ObjectId) -> bool {
        self.looked_up.set(self.looked_up.get() + 1);
        matches!(id.0, 1 | 2)
    }
}

#[test]
fn blog_subtype_as_seven_two_has_it() {
    let blog = MetaType::new("blog").unwrap();
    let single = Rc::new(FixedSites {
        multisite: false,
        looked_up: Cell::new(0),
    });
    let subtypes = SubtypesByType::new().bind("blog", Rc::new(BlogSubtypes::new(single.clone())));
    assert_eq!(subtypes.subtype_of(&blog, ObjectId(1)), "");
    assert_eq!(single.looked_up.get(), 0);
    let multi = Rc::new(FixedSites {
        multisite: true,
        looked_up: Cell::new(0),
    });
    let subtypes = SubtypesByType::new().bind("blog", Rc::new(BlogSubtypes::new(multi.clone())));
    assert_eq!(subtypes.subtype_of(&blog, ObjectId(2)), "blog");
    assert_eq!(subtypes.subtype_of(&blog, ObjectId(99)), "");
    assert_eq!(subtypes.subtype_of(&blog, ObjectId(0)), "");
    assert_eq!(multi.looked_up.get(), 2);
    // An unbound type answers ''.
    assert_eq!(subtypes.subtype_of(&post(), ObjectId(1)), "");
}

#[test]
fn handlers_are_bound() {
    let f = fixture();
    let container = Rc::new(Container::new());
    register_meta_handlers(&container, f.api.clone());
    macro_rules! bound {
        (commands: $($c:ty),*; queries: $($q:ty),*) => {
            $( assert!(container.has(<$c as Command>::KEY), stringify!($c)); )*
            $( assert!(container.has(<$q as Query>::KEY), stringify!($q)); )*
        };
    }
    bound!(
        commands: AddMetadata, UpdateMetadata, DeleteMetadata, UpdateMetadataByMid, DeleteMetadataByMid,
            UpdateMetaCache;
        queries: GetMetadata, GetMetadataRaw, GetMetadataDefault, MetadataExists, GetMetadataByMid, SanitizeMeta,
            IsProtectedMeta, GetObjectSubtype, FilterDefaultMetadata
    );
    assert_eq!(
        <AddMetadata as Command>::KEY,
        "SpeedySpec\\WpMeta\\AddMetadataHandler"
    );
    let commands = CommandBus::new(container.clone());
    let queries = QueryBus::new(container.clone());
    let added = commands
        .dispatch(AddMetadata {
            meta_type: post(),
            object: ObjectId(1),
            key: k("k"),
            value: s("v"),
            unique: false,
        })
        .unwrap()
        .unwrap();
    assert_eq!(added, AddOutcome::Added(MetaId(1)));
    let got = queries
        .ask(GetMetadata {
            meta_type: post(),
            object: ObjectId(1),
            key: Some(k("k")),
            single: true,
        })
        .unwrap()
        .unwrap();
    assert_eq!(got, s("v"));
}

#[test]
fn non_utf8_key_round_trips_through_an_opaque_entry() {
    let f = fixture();
    let key = MetaKey::new(b"\xffk".to_vec());
    let mid = f
        .api
        .add_metadata(&post(), ObjectId(1), &key, s("v"), false)
        .unwrap();
    assert_eq!(mid, AddOutcome::Added(MetaId(1)));
    f.calls.reset();
    let get = || {
        f.api
            .get_metadata(&post(), ObjectId(1), Some(&key), true)
            .unwrap()
    };
    assert_eq!(get(), s("v"));
    assert_eq!(get(), s("v"));
    assert_eq!(f.calls.total(), 1);
    assert!(matches!(cached(&f, &post(), 1), Some(HookValue::Opaque(_))));
    assert!(f.api.metadata_exists(&post(), ObjectId(1), &key).unwrap());
}
