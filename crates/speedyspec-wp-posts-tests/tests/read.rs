//! The post read use cases (`PostsApi`) over `MemoryPostRepository` (counted), `WpHookApi` over the memory store,
//! the Rust object cache, phase 3's `MetadataApi` over memory meta, the registries `create_initial_post_types()`
//! fills, and fixed port fakes, following WordPress 7.2-RC2's `post.php`. Cache keys, values and hook orders are
//! the ones compat/posts-read recorded.

mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_cqrs::{Command, Query};
use speedyspec_php_serialize::{PhpKey, PhpValue, from_hook_value, to_hook_value};
use speedyspec_wp_hooks::WpHookApi;
use speedyspec_wp_hooks_contracts::{Callback, HookError, HookName, HookValue};
use speedyspec_wp_meta::{MetaDeps, MetaKeyRegistry, MetadataApi, UnfilteredProtection};
use speedyspec_wp_meta_contracts::{
    Installing, MetaKey, MetaType, Metadata, ObjectId, ObjectSubtypes,
};
use speedyspec_wp_meta_memory::MemoryMetaRepositories;
use speedyspec_wp_object_cache::{ObjectCachePorts, WpObjectCache};
use speedyspec_wp_posts::{
    CacheInvalidation, CleanPostCache, GetPost, InitialOptions, Microtime, PostEntry, PostRead,
    PostStatusRegistry, PostTypePorts, PostTypeRegistry, PostsApi, PostsDeps, PrimePostCaches,
    Taxonomy, Untranslated, UpdatePostCaches, UpdatePostParentCaches, Users, WpCountPosts,
    create_initial_post_types, read_hooks, register_posts_handlers,
};
use speedyspec_wp_posts_contracts::{
    CurrentPost, Filter, LastPostField, MimeFilter, MimeType, PageRow, Post, PostDateTime,
    PostError, PostId, PostRef, PostRepository, PostStatusName, PostTypeArg, PostTypeName, Posts,
    RewriteContext, Slug, Timezone, TitleSanitizer,
};
use speedyspec_wp_posts_memory::MemoryPostRepository;

// ---- fixed ports and a counting repository ----

/// Every repository call, by name and arguments.
type Calls = Rc<RefCell<Vec<String>>>;

struct Counting {
    inner: MemoryPostRepository,
    calls: Calls,
}

impl Counting {
    fn note(&self, call: String) {
        self.calls.borrow_mut().push(call);
    }
}

impl PostRepository for Counting {
    fn engine(&self) -> String {
        self.inner.engine()
    }
    fn find(&self, id: PostId) -> Result<Option<Post>, PostError> {
        self.note(format!("find({})", id.0));
        self.inner.find(id)
    }
    fn insert(&self, post: &Post) -> Result<Option<PostId>, PostError> {
        self.inner.insert(post)
    }
    fn attachment_slug_owner(
        &self,
        slug: &Slug,
        except: PostId,
    ) -> Result<Option<Slug>, PostError> {
        self.inner.attachment_slug_owner(slug, except)
    }
    fn hierarchical_slug_owner(
        &self,
        slug: &Slug,
        post_type: &PostTypeName,
        except: PostId,
        parent: PostId,
    ) -> Result<Option<Slug>, PostError> {
        self.inner
            .hierarchical_slug_owner(slug, post_type, except, parent)
    }
    fn flat_slug_owner(
        &self,
        slug: &Slug,
        post_type: &PostTypeName,
        except: PostId,
    ) -> Result<Option<Slug>, PostError> {
        self.inner.flat_slug_owner(slug, post_type, except)
    }
    fn find_many(&self, ids: &[PostId]) -> Result<Vec<Post>, PostError> {
        let list: Vec<String> = ids.iter().map(|i| i.0.to_string()).collect();
        self.note(format!("find_many({})", list.join(",")));
        self.inner.find_many(ids)
    }
    fn parent_ids(&self, ids: &[PostId]) -> Result<Vec<(PostId, PostId)>, PostError> {
        let list: Vec<String> = ids.iter().map(|i| i.0.to_string()).collect();
        self.note(format!("parent_ids({})", list.join(",")));
        self.inner.parent_ids(ids)
    }
    fn pages_by_names(
        &self,
        names: &[Slug],
        post_types: &[PostTypeName],
    ) -> Result<Vec<PageRow>, PostError> {
        self.note("pages_by_names".into());
        self.inner.pages_by_names(names, post_types)
    }
    fn all_page_ids(&self) -> Result<Vec<PostId>, PostError> {
        self.note("all_page_ids".into());
        self.inner.all_page_ids()
    }
    fn count_by_status(
        &self,
        post_type: &PostTypeName,
        reader: Option<u64>,
    ) -> Result<Vec<(PostStatusName, u64)>, PostError> {
        self.note(format!("count_by_status({post_type},{reader:?})"));
        self.inner.count_by_status(post_type, reader)
    }
    fn count_attachments(
        &self,
        mime_type: Option<&MimeFilter>,
    ) -> Result<(Vec<(MimeType, u64)>, u64), PostError> {
        self.note("count_attachments".into());
        self.inner.count_attachments(mime_type)
    }
    fn last_post_time(
        &self,
        field: LastPostField,
        post_types: &[PostTypeName],
        timezone: Timezone,
    ) -> Result<Option<PostDateTime>, PostError> {
        self.note("last_post_time".into());
        self.inner.last_post_time(field, post_types, timezone)
    }
    fn available_mime_types(&self, post_type: &PostTypeName) -> Result<Vec<MimeType>, PostError> {
        self.inner.available_mime_types(post_type)
    }
}

/// The global `$post`.
#[derive(Default)]
struct Global(RefCell<Option<PhpValue>>);
impl CurrentPost for Global {
    fn current(&self) -> Option<PhpValue> {
        self.0.borrow().clone()
    }
}

/// A user id (0 is logged out) and whether it may read private posts.
#[derive(Default)]
struct Reader {
    id: Cell<u64>,
    can_read_private: Cell<bool>,
    asked: RefCell<Vec<PhpValue>>,
}
impl Users for Reader {
    fn is_user_logged_in(&self) -> Result<bool, PostError> {
        Ok(self.id.get() != 0)
    }
    fn get_current_user_id(&self) -> Result<u64, PostError> {
        Ok(self.id.get())
    }
    fn current_user_can(&self, capability: &PhpValue) -> Result<bool, PostError> {
        self.asked.borrow_mut().push(capability.clone());
        Ok(self.can_read_private.get())
    }
}

/// Records each terms call.
#[derive(Default)]
struct Terms(RefCell<Vec<String>>);
impl Taxonomy for Terms {
    fn update_object_term_cache(
        &self,
        ids: &[PostId],
        types: &[PhpValue],
    ) -> Result<(), PostError> {
        let ids: Vec<String> = ids.iter().map(|i| i.0.to_string()).collect();
        let types: Vec<String> = types
            .iter()
            .map(|t| {
                String::from_utf8_lossy(&speedyspec_php_serialize::semantics::to_php_string(t))
                    .into_owned()
            })
            .collect();
        self.0
            .borrow_mut()
            .push(format!("update({};{})", ids.join(","), types.join(",")));
        Ok(())
    }
    fn clean_object_term_cache(&self, ids: &[PostId], object_type: &str) -> Result<(), PostError> {
        self.0
            .borrow_mut()
            .push(format!("clean({};{object_type})", ids[0].0));
        Ok(())
    }
    fn post_term_fields(
        &self,
        _: &PhpValue,
        taxonomy: &str,
        field: &str,
    ) -> Result<PhpValue, PostError> {
        Ok(s(&format!("{taxonomy}.{field}")))
    }
}

/// `microtime()`: `0.00000000 1700000000`, then one second later each call.
#[derive(Default)]
struct Clock(Cell<u64>);
impl Microtime for Clock {
    fn microtime(&self) -> String {
        self.0.set(self.0.get() + 1);
        format!("0.00000000 {}", 1_700_000_000 + self.0.get())
    }
}

#[derive(Default)]
struct Suspend(Cell<bool>);
impl CacheInvalidation for Suspend {
    fn suspended(&self) -> bool {
        self.0.get()
    }
}

struct PostSubtype;
impl ObjectSubtypes for PostSubtype {
    fn subtype_of(&self, _: &MetaType, _: ObjectId) -> String {
        "post".into()
    }
}

struct NotInstalling;
impl Installing for NotInstalling {
    fn installing(&self) -> bool {
        false
    }
}

struct NoRewrite;
impl RewriteContext for NoRewrite {
    fn rewrite_applies(&self) -> bool {
        false
    }
}

struct Same;
impl TitleSanitizer for Same {
    fn sanitize_title_with_dashes(&self, title: &[u8]) -> Vec<u8> {
        title.to_vec()
    }
}

/// Each hook run, with its arguments.
type Log = Rc<RefCell<Vec<(String, Vec<PhpValue>)>>>;

struct Fixture {
    api: Rc<PostsApi>,
    hooks: Rc<WpHookApi>,
    cache: Rc<WpObjectCache>,
    metadata: Rc<MetadataApi>,
    calls: Calls,
    global: Rc<Global>,
    users: Rc<Reader>,
    terms: Rc<Terms>,
    suspend: Rc<Suspend>,
    log: Log,
}

fn post(id: u64, post_type: &str, name: &str, parent: u64, status: &str, author: u64) -> Post {
    Post {
        id: PostId(id),
        author,
        title: format!("Title {id}").into_bytes(),
        status: PostStatusName::new(status),
        name: Slug::from(name),
        parent: PostId(parent),
        menu_order: id as i64,
        post_type: PostTypeName::new(post_type),
        date: speedyspec_wp_posts_contracts::PostDate {
            local: PostDateTime::new(format!("2026-01-{id:02} 10:00:00")),
            gmt: PostDateTime::new(format!("2026-01-{id:02} 08:00:00")),
        },
        ..Post::default()
    }
}

fn fixture() -> Fixture {
    fixture_with(vec![
        post(1, "post", "hello-world", 0, "publish", 1),
        post(2, "page", "about", 0, "publish", 1),
        post(3, "page", "team", 2, "publish", 1),
        post(4, "post", "secret", 0, "private", 1),
        post(5, "post", "mine", 0, "private", 2),
        post(6, "post", "draft", 0, "draft", 2),
        post(9, "attachment", "photo", 1, "inherit", 1),
    ])
}

fn fixture_with(posts: Vec<Post>) -> Fixture {
    let hooks = support::hooks();
    let cache = Rc::new(WpObjectCache::new(ObjectCachePorts::rust(), None));
    let repository = MemoryPostRepository::new();
    for p in posts {
        repository.insert(&p).unwrap();
    }
    let calls: Calls = Rc::default();
    let metadata = MetadataApi::new(MetaDeps {
        hooks: hooks.clone(),
        cache: cache.clone(),
        repositories: Rc::new(MemoryMetaRepositories::new(&["post"])),
        keys: Rc::new(MetaKeyRegistry::new()),
        subtypes: Rc::new(PostSubtype),
        protection: Rc::new(UnfilteredProtection),
        installing: Rc::new(NotInstalling),
    });
    let mut types = PostTypeRegistry::new();
    let mut statuses = PostStatusRegistry::new();
    create_initial_post_types(
        &mut types,
        &mut statuses,
        &InitialOptions {
            attachment_pages_enabled: false,
        },
        &PostTypePorts {
            translator: &Untranslated,
            rewrite: &NoRewrite,
            titles: &Same,
        },
    );
    let global: Rc<Global> = Rc::default();
    let users: Rc<Reader> = Rc::default();
    let terms: Rc<Terms> = Rc::default();
    let suspend: Rc<Suspend> = Rc::default();
    let api = PostsApi::new(PostsDeps {
        hooks: hooks.clone(),
        cache: cache.clone(),
        repository: Rc::new(Counting {
            inner: repository,
            calls: calls.clone(),
        }),
        metadata: metadata.clone(),
        types: Rc::new(types),
        statuses: Rc::new(statuses),
        current: global.clone(),
        formatting: Rc::new(support::RustFormatting::new(hooks.clone())),
        users: users.clone(),
        terms: terms.clone(),
        clock: Rc::new(Clock::default()),
        invalidation: suspend.clone(),
        options: Rc::new(NoOptions),
    });
    Fixture {
        api,
        hooks,
        cache,
        metadata,
        calls,
        global,
        users,
        terms,
        suspend,
        log: Rc::default(),
    }
}

fn s(text: &str) -> PhpValue {
    PhpValue::Str(text.as_bytes().to_vec())
}

fn hs(text: &str) -> HookValue {
    HookValue::Str(text.into())
}

fn group(name: &str) -> HookValue {
    hs(name)
}

fn cached(f: &Fixture, key: HookValue, group_name: &str) -> Option<PhpValue> {
    f.cache
        .peek(&key, &group(group_name))
        .map(|v| from_hook_value(&v).unwrap())
}

fn calls(f: &Fixture) -> Vec<String> {
    f.calls.borrow().clone()
}

fn property<'a>(object: &'a PhpValue, name: &str) -> Option<&'a PhpValue> {
    match object {
        PhpValue::Object { properties, .. } => {
            speedyspec_php_serialize::semantics::get(properties, name)
        }
        _ => None,
    }
}

/// Logs each run of `hook` with its arguments, at priority 10; the callback answers `answer(args)`.
fn on(f: &Fixture, hook: &str, answer: impl Fn(&[HookValue]) -> HookValue + 'static) {
    let log = f.log.clone();
    let name = hook.to_owned();
    f.hooks
        .add_filter(
            &HookName::from(hook),
            Callback::rust(&format!("on_{hook}"), move |args: &[HookValue]| {
                let args_php = args.iter().map(|a| from_hook_value(a).unwrap()).collect();
                log.borrow_mut().push((name.clone(), args_php));
                Ok::<_, HookError>(answer(args))
            }),
            &HookValue::Int(10),
            99,
        )
        .unwrap();
}

/// Logs each run of `hook`, answering its first argument.
fn record(f: &Fixture, hook: &str) {
    on(f, hook, |args| {
        args.first().cloned().unwrap_or(HookValue::Null)
    });
}

fn fired(f: &Fixture) -> Vec<String> {
    f.log.borrow().iter().map(|(h, _)| h.clone()).collect()
}

fn args_of(f: &Fixture, hook: &str) -> Vec<PhpValue> {
    f.log
        .borrow()
        .iter()
        .find(|(h, _)| h == hook)
        .map(|(_, a)| a.clone())
        .unwrap_or_else(|| panic!("{hook} did not run"))
}

/// A callback on `hook` that fails with a payload, returned for comparison.
fn failing(f: &Fixture, hook: &str) -> Rc<dyn std::any::Any> {
    let payload: Rc<dyn std::any::Any> = Rc::new(format!("exception from {hook}"));
    let kept = payload.clone();
    f.hooks
        .add_filter(
            &HookName::from(hook),
            Callback::rust(&format!("fail_{hook}"), move |_: &[HookValue]| {
                Err::<HookValue, _>(HookError {
                    message: "thrown".into(),
                    payload: Some(kept.clone()),
                })
            }),
            &HookValue::Int(10),
            1,
        )
        .unwrap();
    payload
}

fn same_payload(error: PostError, payload: &Rc<dyn std::any::Any>) {
    match error {
        PostError::Hook(e) => assert!(Rc::ptr_eq(e.payload.as_ref().unwrap(), payload)),
        other => panic!("expected a hook error, got {other:?}"),
    }
}

// ---- get_post ----

#[test]
fn get_post_zero_reads_current_post() {
    let f = fixture();
    // No global: get_post( 0 ) and get_post() are null, with no statement.
    assert_eq!(
        f.api.get(PostRef::Id(PostId(0)), Filter::Raw).unwrap(),
        None
    );
    assert_eq!(f.api.get(PostRef::Current, Filter::Raw).unwrap(), None);
    assert!(calls(&f).is_empty());

    // A raw WP_Post global is used as it is: its properties, not the table's, and no statement.
    let mut global = PostEntry::raw_properties(&post(1, "post", "hello-world", 0, "publish", 1));
    speedyspec_php_serialize::semantics::set(&mut global, "post_title", s("Global"));
    *f.global.0.borrow_mut() = Some(PostEntry::wp_post(global));
    for zero in [PostRef::Id(PostId(0)), PostRef::Current] {
        let read = f.api.get(zero, Filter::Raw).unwrap().unwrap();
        assert_eq!(read.id, PostId(1));
        assert_eq!(read.title, b"Global");
    }
    assert!(calls(&f).is_empty());
    // An id is not the global.
    assert_eq!(
        f.api
            .get(PostRef::Id(PostId(2)), Filter::Raw)
            .unwrap()
            .unwrap()
            .title,
        b"Title 2"
    );

    // A WP_Post global filtered for display is re-read by its ID (WP_Post::filter( 'raw' )).
    let mut display = PostEntry::raw_properties(&post(3, "page", "team", 2, "publish", 1));
    speedyspec_php_serialize::semantics::set(&mut display, "post_title", s("Escaped &amp;"));
    speedyspec_php_serialize::semantics::set(&mut display, "filter", s("display"));
    *f.global.0.borrow_mut() = Some(PostEntry::wp_post(display));
    assert_eq!(
        f.api
            .get(PostRef::Current, Filter::Raw)
            .unwrap()
            .unwrap()
            .title,
        b"Title 3"
    );

    // A numeric global is an id; a non-numeric one is null (get-global-not-a-post).
    *f.global.0.borrow_mut() = Some(PhpValue::Int(2));
    assert_eq!(
        f.api
            .get(PostRef::Current, Filter::Raw)
            .unwrap()
            .unwrap()
            .id,
        PostId(2)
    );
    *f.global.0.borrow_mut() = Some(s("abc"));
    assert_eq!(f.api.get(PostRef::Current, Filter::Raw).unwrap(), None);

    // The readers beside get_post() fall back the same way.
    *f.global.0.borrow_mut() = Some(PhpValue::Int(3));
    assert_eq!(f.api.parent_id(PostRef::Current).unwrap(), Some(PostId(2)));
    assert_eq!(
        f.api.page_uri(PostRef::Current).unwrap(),
        Some(b"about/team".to_vec())
    );
    assert_eq!(
        f.api
            .field("post_name", PostRef::Id(PostId(0)), Filter::Display)
            .unwrap(),
        s("team")
    );
}

#[test]
fn reads_php_shaped_post_entry() {
    let f = fixture();
    // A stdClass another PHP process cached: string numbers, filter raw, a property the table does not have.
    let entry = PhpValue::Object {
        class: "stdClass".into(),
        properties: vec![
            (PhpKey::Str(b"ID".to_vec()), PhpValue::Int(7)),
            (PhpKey::Str(b"post_author".to_vec()), s("3")),
            (PhpKey::Str(b"post_title".to_vec()), s("From PHP")),
            (PhpKey::Str(b"post_status".to_vec()), s("publish")),
            (PhpKey::Str(b"post_name".to_vec()), s("from-php")),
            (PhpKey::Str(b"post_parent".to_vec()), PhpValue::Int(2)),
            (PhpKey::Str(b"post_type".to_vec()), s("page")),
            (PhpKey::Str(b"comment_count".to_vec()), s("4")),
            (PhpKey::Str(b"extra".to_vec()), s("kept")),
            (PhpKey::Str(b"filter".to_vec()), s("raw")),
        ],
    };
    f.cache
        .set(
            &HookValue::Int(7),
            to_hook_value(&entry),
            &group("posts"),
            0,
        )
        .unwrap();
    let read = f
        .api
        .get(PostRef::Id(PostId(7)), Filter::Raw)
        .unwrap()
        .unwrap();
    assert_eq!(read.id, PostId(7));
    assert_eq!(read.author, 3);
    assert_eq!(read.title, b"From PHP");
    assert_eq!(read.parent, PostId(2));
    assert_eq!(read.comment_count, 4);
    assert_eq!(read.post_type.as_str(), "page");
    assert!(calls(&f).is_empty(), "no statement: {:?}", calls(&f));
    // The entry is left as PHP wrote it, and its other properties are WP_Post's.
    assert_eq!(cached(&f, HookValue::Int(7), "posts"), Some(entry));
    let object = f
        .api
        .get_post_object(PostRef::Id(PostId(7)))
        .unwrap()
        .unwrap();
    assert_eq!(property(&object, "extra"), Some(&s("kept")));
    assert_eq!(
        f.api.page_uri(PostRef::Id(PostId(7))).unwrap(),
        Some(b"about/from-php".to_vec())
    );

    f.calls.borrow_mut().clear();
    // An unfiltered entry is sanitized raw first: (int) on ID, post_parent and menu_order (cache-php-entry-unfiltered).
    let unfiltered = PhpValue::Object {
        class: "stdClass".into(),
        properties: vec![
            (PhpKey::Str(b"ID".to_vec()), s("8")),
            (PhpKey::Str(b"post_parent".to_vec()), s("3")),
            (PhpKey::Str(b"post_title".to_vec()), s("<b>raw</b>")),
        ],
    };
    f.cache
        .set(
            &HookValue::Int(8),
            to_hook_value(&unfiltered),
            &group("posts"),
            0,
        )
        .unwrap();
    let object = f
        .api
        .get_post_object(PostRef::Id(PostId(8)))
        .unwrap()
        .unwrap();
    assert_eq!(property(&object, "ID"), Some(&PhpValue::Int(8)));
    assert_eq!(property(&object, "post_parent"), Some(&PhpValue::Int(3)));
    assert_eq!(property(&object, "post_title"), Some(&s("<b>raw</b>")));
    assert_eq!(property(&object, "filter"), Some(&s("raw")));
    assert!(calls(&f).is_empty());
}

#[test]
fn non_object_entry_is_a_miss() {
    let f = fixture();
    for (id, bad) in [
        (1u64, s("stale")),
        (
            2,
            PhpValue::Array(vec![(PhpKey::Str(b"ID".to_vec()), PhpValue::Int(2))]),
        ),
        (
            3,
            PhpValue::Object {
                class: "stdClass".into(),
                properties: vec![(PhpKey::Str(b"post_title".to_vec()), s("no ID"))],
            },
        ),
        (
            9,
            PhpValue::Object {
                class: "ArrayObject".into(),
                properties: vec![(PhpKey::Str(b"ID".to_vec()), PhpValue::Int(9))],
            },
        ),
    ] {
        let key = HookValue::Int(id as i64);
        f.cache
            .set(&key, to_hook_value(&bad), &group("posts"), 0)
            .unwrap();
        let read = f
            .api
            .get(PostRef::Id(PostId(id)), Filter::Raw)
            .unwrap()
            .unwrap();
        assert_eq!(read.id, PostId(id));
        // Replaced with wp_cache_set(), not add: the row's raw entry is there now.
        assert_eq!(cached(&f, key, "posts"), Some(PostEntry::to_entry(&read)));
    }
    assert_eq!(calls(&f), ["find(1)", "find(2)", "find(3)", "find(9)"]);
}

#[test]
fn get_post_caches_on_miss_once() {
    let f = fixture();
    let first = f
        .api
        .get(PostRef::Id(PostId(1)), Filter::Raw)
        .unwrap()
        .unwrap();
    let second = f
        .api
        .get(PostRef::Id(PostId(1)), Filter::Raw)
        .unwrap()
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(calls(&f), ["find(1)"]);
    // The entry is the recorded shape: ints for ID, post_parent and menu_order, strings otherwise, filter raw.
    let entry = cached(&f, HookValue::Int(1), "posts").unwrap();
    assert_eq!(entry, PostEntry::to_entry(&first));
    assert!(matches!(&entry, PhpValue::Object { class, .. } if class == "stdClass"));
    assert_eq!(property(&entry, "post_author"), Some(&s("1")));
    assert_eq!(property(&entry, "menu_order"), Some(&PhpValue::Int(1)));
    assert_eq!(property(&entry, "comment_count"), Some(&s("0")));
    assert_eq!(property(&entry, "filter"), Some(&s("raw")));

    // A missing post is not cached: each read queries (get-missing).
    f.calls.borrow_mut().clear();
    assert_eq!(
        f.api.get(PostRef::Id(PostId(999)), Filter::Raw).unwrap(),
        None
    );
    assert_eq!(
        f.api.get(PostRef::Id(PostId(999)), Filter::Raw).unwrap(),
        None
    );
    assert_eq!(calls(&f), ["find(999)", "find(999)"]);

    // The readers share the entry.
    f.calls.borrow_mut().clear();
    assert_eq!(
        f.api
            .post_type(PostRef::Id(PostId(1)))
            .unwrap()
            .unwrap()
            .as_str(),
        "post"
    );
    assert_eq!(
        f.api.ancestors(PostRef::Id(PostId(3))).unwrap(),
        [PostId(2)]
    );
    assert_eq!(calls(&f), ["find(3)", "find(2)"]);
    // An attachment inheriting from a published parent is published, through the get_post_status filter.
    record(&f, "get_post_status");
    assert_eq!(
        f.api
            .status(PostRef::Id(PostId(9)))
            .unwrap()
            .unwrap()
            .as_str(),
        "publish"
    );
    // The parent's status runs twice (the trash check, then its value), then the attachment's own.
    assert_eq!(
        fired(&f),
        ["get_post_status", "get_post_status", "get_post_status"]
    );
}

// ---- get_page_by_path ----

#[test]
fn page_by_path_uses_salted_cache() {
    let f = fixture();
    let page = PostTypeArg::One(PostTypeName::new("page"));
    let found = f.api.page_by_path("about/team", &page).unwrap().unwrap();
    assert_eq!(found.id, PostId(3));
    // The key and salt as recorded (page-path-cache): md5( $path . serialize( 'page' ) ), the ID as a string.
    let key = hs("get_page_by_path:291347b13b493a4537ef2fcd911be425");
    let salt = cached(&f, hs("last_changed"), "posts").unwrap();
    assert_eq!(salt, s("0.00000000 1700000001"));
    assert_eq!(
        cached(&f, key.clone(), "post-queries"),
        Some(PhpValue::Array(vec![
            (PhpKey::Str(b"data".to_vec()), s("3")),
            (PhpKey::Str(b"salt".to_vec()), salt.clone()),
        ]))
    );
    assert_eq!(calls(&f), ["pages_by_names", "find(3)"]);

    // A hit sends no statement; a miss is cached as 0.
    f.calls.borrow_mut().clear();
    assert_eq!(
        f.api.page_by_path("about/team", &page).unwrap().unwrap().id,
        PostId(3)
    );
    assert_eq!(f.api.page_by_path("nope", &page).unwrap(), None);
    assert_eq!(f.api.page_by_path("nope", &page).unwrap(), None);
    assert_eq!(calls(&f), ["pages_by_names"]);
    assert_eq!(
        cached(
            &f,
            hs("get_page_by_path:89b57f44d88ecc0737e96622a191d046"),
            "post-queries"
        ),
        Some(PhpValue::Array(vec![
            (PhpKey::Str(b"data".to_vec()), PhpValue::Int(0)),
            (PhpKey::Str(b"salt".to_vec()), salt),
        ]))
    );

    // clean_post_cache() moves last_changed, so the old entry no longer counts.
    f.api.clean_cache(PostId(2)).unwrap();
    f.calls.borrow_mut().clear();
    assert_eq!(
        f.api.page_by_path("about/team", &page).unwrap().unwrap().id,
        PostId(3)
    );
    assert_eq!(calls(&f), ["pages_by_names"]);

    // Case: the parts go through sanitize_title, and the match is ===, so "About" finds nothing without a
    // lower-casing filter, and finds about with one.
    assert_eq!(f.api.page_by_path("About", &page).unwrap(), None);
    // The miss is cached under the same salt, so the filter only counts once last_changed moves.
    on(&f, "sanitize_title", |args| match &args[0] {
        HookValue::Str(t) => HookValue::Str(t.to_lowercase()),
        other => other.clone(),
    });
    assert_eq!(f.api.page_by_path("About", &page).unwrap(), None);
    f.api.wp_cache_set_posts_last_changed().unwrap();
    assert_eq!(
        f.api.page_by_path("About", &page).unwrap().unwrap().id,
        PostId(2)
    );
    assert_eq!(
        args_of(&f, "sanitize_title"),
        [s("About"), s("About"), s("query")]
    );
    // A path that is not the full chain from the top does not match.
    assert_eq!(f.api.page_by_path("team", &page).unwrap(), None);
}

// ---- wp_count_posts ----

fn counts(object: &PhpValue) -> Vec<(String, PhpValue)> {
    match object {
        PhpValue::Object { properties, .. } => properties
            .iter()
            .map(|(k, v)| {
                (
                    speedyspec_php_serialize::semantics::key_to_string(k),
                    v.clone(),
                )
            })
            .collect(),
        other => panic!("not an object: {other:?}"),
    }
}

fn count_of(object: &PhpValue, status: &str) -> PhpValue {
    property(object, status).cloned().unwrap_or(PhpValue::Null)
}

#[test]
fn count_posts_readable_adds_private_for_author() {
    let f = fixture();
    let post = PostTypeName::new("post");
    // Logged out: the plain statement and key, every private post counted.
    let all = f.api.count_posts(&post, "readable").unwrap();
    assert_eq!(count_of(&all, "private"), s("2"));
    assert!(cached(&f, hs("posts-post"), "counts").is_some());

    // An author who cannot read private posts: the UNION statement, the reader's own private posts, its own key.
    f.users.id.set(2);
    let out = f.api.count_posts(&post, "readable").unwrap();
    assert_eq!(count_of(&out, "private"), s("1"));
    assert_eq!(count_of(&out, "publish"), s("1"));
    assert_eq!(count_of(&out, "draft"), s("1"));
    // Statuses with no posts are int 0, in get_post_stati() order (count-readable-author).
    assert_eq!(count_of(&out, "pending"), PhpValue::Int(0));
    let names: Vec<String> = counts(&out).into_iter().map(|(k, _)| k).collect();
    assert_eq!(
        names,
        [
            "publish",
            "future",
            "draft",
            "pending",
            "private",
            "trash",
            "auto-draft",
            "inherit",
            "request-pending",
            "request-confirmed",
            "request-failed",
            "request-completed"
        ]
    );
    assert_eq!(
        cached(&f, hs("posts-post_readable_2"), "counts"),
        Some(out.clone())
    );
    assert_eq!(
        calls(&f),
        [
            "count_by_status(post,None)",
            "count_by_status(post,Some(2))"
        ]
    );
    // The capability asked is the type's read_private_posts, twice: the key, then the statement.
    assert_eq!(
        *f.users.asked.borrow(),
        [s("read_private_posts"), s("read_private_posts")]
    );

    // The second read is the cache; an admin shares the plain key.
    f.calls.borrow_mut().clear();
    assert_eq!(f.api.count_posts(&post, "readable").unwrap(), out);
    f.users.can_read_private.set(true);
    assert_eq!(
        count_of(&f.api.count_posts(&post, "readable").unwrap(), "private"),
        s("2")
    );
    assert!(calls(&f).is_empty());

    // An unknown type is an empty stdClass, with no statement.
    assert_eq!(
        f.api.count_posts(&PostTypeName::new("nope"), "").unwrap(),
        PhpValue::Object {
            class: "stdClass".into(),
            properties: Vec::new()
        }
    );
}

#[test]
fn count_posts_filter_runs_last() {
    let f = fixture();
    record(&f, "pre_wp_count_posts");
    on(&f, "wp_count_posts", |_| HookValue::Str("filtered".into()));
    let post = PostTypeName::new("post");
    assert_eq!(f.api.count_posts(&post, "").unwrap(), s("filtered"));
    assert_eq!(fired(&f), ["pre_wp_count_posts", "wp_count_posts"]);
    assert_eq!(
        args_of(&f, "pre_wp_count_posts"),
        [PhpValue::Null, s("post"), s("")]
    );
    // The filter gets the counts the cache holds, then the type and perm; the cache keeps the unfiltered counts.
    let unfiltered = cached(&f, hs("posts-post"), "counts").unwrap();
    assert_eq!(
        args_of(&f, "wp_count_posts"),
        [unfiltered.clone(), s("post"), s("")]
    );
    assert_eq!(count_of(&unfiltered, "publish"), s("1"));

    // A stdClass from pre_wp_count_posts skips the cache and statement, gains the missing statuses as 0, and is
    // still filtered (count-pre-filter).
    let f = fixture();
    on(&f, "pre_wp_count_posts", |_| {
        to_hook_value(&PhpValue::Object {
            class: "stdClass".into(),
            properties: vec![(PhpKey::Str(b"publish".to_vec()), PhpValue::Int(42))],
        })
    });
    record(&f, "wp_count_posts");
    let out = f.api.count_posts(&post, "").unwrap();
    assert_eq!(count_of(&out, "publish"), PhpValue::Int(42));
    assert_eq!(count_of(&out, "trash"), PhpValue::Int(0));
    assert_eq!(fired(&f), ["pre_wp_count_posts", "wp_count_posts"]);
    assert!(calls(&f).is_empty());
    assert_eq!(cached(&f, hs("posts-post"), "counts"), None);

    // A cached count from before a status was registered gains it as 0 (count-new-status).
    let f = fixture();
    f.cache
        .set(
            &hs("posts-post"),
            to_hook_value(&PhpValue::Object {
                class: "stdClass".into(),
                properties: vec![(PhpKey::Str(b"publish".to_vec()), s("9"))],
            }),
            &group("counts"),
            0,
        )
        .unwrap();
    let out = f.api.count_posts(&post, "").unwrap();
    assert_eq!(count_of(&out, "publish"), s("9"));
    assert_eq!(count_of(&out, "inherit"), PhpValue::Int(0));
    assert!(calls(&f).is_empty());
}

// ---- the post caches ----

#[test]
fn clean_post_cache_fires_actions_and_bumps_last_changed() {
    let f = fixture();
    assert_eq!(f.api.all_page_ids().unwrap(), [PostId(2), PostId(3)]);
    assert_eq!(
        cached(&f, hs("all_page_ids"), "posts"),
        Some(PhpValue::Array(vec![
            (PhpKey::Int(0), s("2")),
            (PhpKey::Int(1), s("3"))
        ]))
    );
    f.api.get(PostRef::Id(PostId(2)), Filter::Raw).unwrap();
    f.api.prime_parent_caches(&[PostId(2)]).unwrap();
    f.metadata
        .add(
            &MetaType::new("post").unwrap(),
            ObjectId(2),
            &MetaKey::from("k"),
            s("v"),
            false,
        )
        .unwrap();
    f.metadata
        .get(&MetaType::new("post").unwrap(), ObjectId(2), None, false)
        .unwrap();
    assert!(cached(&f, HookValue::Int(2), "post_meta").is_some());
    assert_eq!(
        cached(&f, hs("post_parent:2"), "posts"),
        Some(PhpValue::Int(0))
    );
    record(&f, "clean_post_cache");
    record(&f, "clean_page_cache");
    record(&f, "wp_cache_set_last_changed");

    f.api.clean_cache(PostId(2)).unwrap();
    assert_eq!(
        fired(&f),
        [
            "clean_post_cache",
            "clean_page_cache",
            "wp_cache_set_last_changed"
        ]
    );
    // ( $post->ID, $post ) with the WP_Post as recorded (cache-clean-page), then ( $post->ID ).
    let args = args_of(&f, "clean_post_cache");
    assert_eq!(args[0], PhpValue::Int(2));
    assert!(matches!(&args[1], PhpValue::Object { class, .. } if class == "WP_Post"));
    assert_eq!(property(&args[1], "filter"), Some(&s("raw")));
    assert_eq!(args_of(&f, "clean_page_cache"), [PhpValue::Int(2)]);
    assert_eq!(
        args_of(&f, "wp_cache_set_last_changed"),
        [
            s("posts"),
            s("0.00000000 1700000001"),
            PhpValue::Bool(false)
        ]
    );
    for (key, group_name) in [
        (HookValue::Int(2), "posts"),
        (hs("post_parent:2"), "posts"),
        (HookValue::Int(2), "post_meta"),
        (hs("all_page_ids"), "posts"),
    ] {
        assert_eq!(
            cached(&f, key.clone(), group_name),
            None,
            "{key:?} in {group_name}"
        );
    }
    assert_eq!(
        cached(&f, hs("last_changed"), "posts"),
        Some(s("0.00000000 1700000001"))
    );
    assert_eq!(*f.terms.0.borrow(), ["clean(2;page)"]);

    // A post is not a page: no clean_page_cache, all_page_ids stays; last_changed moves again.
    f.log.borrow_mut().clear();
    f.api.all_page_ids().unwrap();
    f.api.clean_cache(PostId(1)).unwrap();
    assert_eq!(fired(&f), ["clean_post_cache", "wp_cache_set_last_changed"]);
    assert!(cached(&f, hs("all_page_ids"), "posts").is_some());
    assert_eq!(
        cached(&f, hs("last_changed"), "posts"),
        Some(s("0.00000000 1700000002"))
    );

    // Suspended invalidation and a missing post do nothing.
    f.log.borrow_mut().clear();
    f.suspend.0.set(true);
    f.api.clean_cache(PostId(1)).unwrap();
    f.suspend.0.set(false);
    f.api.clean_cache(PostId(999)).unwrap();
    assert!(fired(&f).is_empty());
}

#[test]
fn prime_skips_cached_ids() {
    let f = fixture();
    f.api.get(PostRef::Id(PostId(1)), Filter::Raw).unwrap();
    f.calls.borrow_mut().clear();
    f.api
        .prime_caches(&[PostId(1), PostId(2), PostId(3), PostId(2)], false, false)
        .unwrap();
    assert_eq!(calls(&f), ["find_many(2,3)"]);
    assert_eq!(
        cached(&f, HookValue::Int(3), "posts"),
        Some(PostEntry::to_entry(&post(
            3, "page", "team", 2, "publish", 1
        )))
    );
    f.calls.borrow_mut().clear();
    f.api
        .prime_caches(&[PostId(1), PostId(2), PostId(3)], false, false)
        .unwrap();
    assert!(calls(&f).is_empty());
    assert!(f.terms.0.borrow().is_empty());

    // With meta, update_postmeta_cache() for every id given; with terms, the distinct types, false for no post.
    f.metadata
        .add(
            &MetaType::new("post").unwrap(),
            ObjectId(1),
            &MetaKey::from("k"),
            s("v"),
            false,
        )
        .unwrap();
    f.api
        .prime_caches(&[PostId(1), PostId(2), PostId(999)], true, true)
        .unwrap();
    assert!(cached(&f, HookValue::Int(1), "post_meta").is_some());
    assert!(cached(&f, HookValue::Int(999), "post_meta").is_some());
    assert_eq!(*f.terms.0.borrow(), ["update(1,2,999;post,page,)"]);

    // Parent ids: only the missing keys are read, each cached as an int.
    f.calls.borrow_mut().clear();
    f.api
        .prime_parent_caches(&[PostId(3), PostId(3), PostId(1)])
        .unwrap();
    f.api.prime_parent_caches(&[PostId(3), PostId(9)]).unwrap();
    assert_eq!(calls(&f), ["parent_ids(3,1)", "parent_ids(9)"]);
    assert_eq!(
        cached(&f, hs("post_parent:3"), "posts"),
        Some(PhpValue::Int(2))
    );
}

#[test]
fn hook_error_stops_before_cache_write() {
    let f = fixture();
    let payload = failing(&f, "sanitize_title");
    let page = PostTypeArg::One(PostTypeName::new("page"));
    same_payload(f.api.page_by_path("about", &page).unwrap_err(), &payload);
    let hash: String = <md5::Md5 as md5::Digest>::digest(b"abouts:4:\"page\";")
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(
        cached(&f, hs(&format!("get_page_by_path:{hash}")), "post-queries"),
        None
    );
    assert!(!calls(&f).contains(&"pages_by_names".to_string()));

    let payload = failing(&f, "pre_wp_count_posts");
    same_payload(
        f.api
            .count_posts(&PostTypeName::new("post"), "")
            .unwrap_err(),
        &payload,
    );
    assert_eq!(cached(&f, hs("posts-post"), "counts"), None);
    assert!(calls(&f).is_empty());

    // A failing clean_post_cache callback stops before clean_page_cache and last_changed.
    // get_page_by_path() read last_changed before its filter failed; the failing clean leaves it alone.
    let before = cached(&f, hs("last_changed"), "posts");
    assert!(before.is_some());
    let payload = failing(&f, "clean_post_cache");
    record(&f, "clean_page_cache");
    same_payload(f.api.clean_cache(PostId(2)).unwrap_err(), &payload);
    assert!(fired(&f).is_empty());
    assert_eq!(cached(&f, hs("last_changed"), "posts"), before);

    // get_post_status' filter error reaches the caller.
    let payload = failing(&f, "get_post_status");
    same_payload(f.api.status(PostRef::Id(PostId(1))).unwrap_err(), &payload);
}

#[test]
fn read_hooks_lists_get_post_metadata_free_path() {
    let names = |read| -> Vec<String> {
        read_hooks(read)
            .iter()
            .map(|n| match n.value() {
                HookValue::Str(s) => s.clone(),
                other => panic!("{other:?}"),
            })
            .collect()
    };
    // get_post() fires no hook and reads no metadata, so it is native whenever the cache and table are.
    assert!(names(PostRead::Get).is_empty());
    assert!(names(PostRead::PostType).is_empty());
    assert!(names(PostRead::Ancestors).is_empty());
    assert!(
        names(PostRead::Field {
            name: b"post_title",
            context: "raw"
        })
        .is_empty()
    );
    assert_eq!(
        names(PostRead::Field {
            name: b"post_title",
            context: "display"
        }),
        ["post_title"]
    );
    assert_eq!(
        names(PostRead::Field {
            name: b"post_content",
            context: "edit"
        }),
        [
            "edit_post_content",
            "content_edit_pre",
            "user_can_richedit",
            "format_to_edit"
        ]
    );
    // A meta key reads post meta.
    assert_eq!(
        names(PostRead::Field {
            name: b"color",
            context: "raw"
        }),
        [
            "get_post_metadata",
            "update_post_metadata_cache",
            "default_post_metadata"
        ]
    );
    // page_template reads _wp_page_template, so a cold post_meta entry runs update_post_metadata_cache.
    assert_eq!(
        names(PostRead::Field {
            name: b"page_template",
            context: "raw"
        }),
        [
            "get_post_metadata",
            "update_post_metadata_cache",
            "default_post_metadata"
        ]
    );
    assert_eq!(
        names(PostRead::UpdatePostParentCaches),
        ["update_post_metadata_cache"]
    );
    assert!(names(PostRead::UpdatePostCaches { meta: false }).is_empty());
    assert_eq!(
        names(PostRead::Status),
        [
            "get_post_metadata",
            "update_post_metadata_cache",
            "default_post_metadata",
            "get_post_status"
        ]
    );
    assert_eq!(
        names(PostRead::CountPosts),
        ["pre_wp_count_posts", "wp_count_posts"]
    );
    assert_eq!(
        names(PostRead::CleanCache),
        [
            "clean_post_cache",
            "clean_page_cache",
            "wp_cache_set_last_changed"
        ]
    );
    assert_eq!(
        names(PostRead::PrimeCaches { meta: true }),
        ["update_post_metadata_cache"]
    );
}

// ---- the readers beside get_post ----

#[test]
fn field_reads_columns_magic_and_meta() {
    let f = fixture();
    let one = PostRef::Id(PostId(1));
    assert_eq!(
        f.api.field("post_title", one, Filter::Raw).unwrap(),
        s("Title 1")
    );
    assert_eq!(
        f.api.field("ID", one, Filter::Display).unwrap(),
        PhpValue::Int(1)
    );
    assert_eq!(f.api.field("nope", one, Filter::Display).unwrap(), s(""));
    assert_eq!(
        f.api
            .field("post_title", PostRef::Id(PostId(999)), Filter::Display)
            .unwrap(),
        s("")
    );
    assert_eq!(
        f.api
            .field("ancestors", PostRef::Id(PostId(3)), Filter::Display)
            .unwrap(),
        PhpValue::Array(vec![(PhpKey::Int(0), PhpValue::Int(2))])
    );
    assert_eq!(
        f.api.field("tags_input", one, Filter::Raw).unwrap(),
        s("post_tag.name")
    );
    let post_meta = MetaType::new("post").unwrap();
    f.metadata
        .add(
            &post_meta,
            ObjectId(1),
            &MetaKey::from("color"),
            s("blue"),
            false,
        )
        .unwrap();
    f.metadata
        .add(
            &post_meta,
            ObjectId(1),
            &MetaKey::from("_wp_page_template"),
            s("tpl.php"),
            false,
        )
        .unwrap();
    record(&f, "post_color");
    assert_eq!(
        f.api.field("color", one, Filter::Display).unwrap(),
        s("blue")
    );
    assert_eq!(
        args_of(&f, "post_color"),
        [s("blue"), PhpValue::Int(1), s("display")]
    );
    assert_eq!(
        f.api.field("page_template", one, Filter::Raw).unwrap(),
        s("tpl.php")
    );
}

#[test]
fn status_follows_a_trashed_parent() {
    let f = fixture();
    let meta = MetaType::new("post").unwrap();
    // Trash the parent of attachment 9 (post 1), remembering it was published as "private".
    let mut trashed = post(1, "post", "hello-world", 0, "trash", 1);
    trashed.title = b"Trashed".to_vec();
    f.cache
        .set(
            &HookValue::Int(1),
            to_hook_value(&PostEntry::to_entry(&trashed)),
            &group("posts"),
            0,
        )
        .unwrap();
    assert_eq!(
        f.api
            .status(PostRef::Id(PostId(9)))
            .unwrap()
            .unwrap()
            .as_str(),
        "publish"
    );
    f.metadata
        .add(
            &meta,
            ObjectId(1),
            &MetaKey::from("_wp_trash_meta_status"),
            s("private"),
            false,
        )
        .unwrap();
    record(&f, "get_post_status");
    assert_eq!(
        f.api
            .status(PostRef::Id(PostId(9)))
            .unwrap()
            .unwrap()
            .as_str(),
        "private"
    );
    // The parent's status runs once for the trash check, then the attachment's own filter.
    assert_eq!(fired(&f), ["get_post_status", "get_post_status"]);
    assert_eq!(f.api.status(PostRef::Id(PostId(999))).unwrap(), None);
}

#[test]
fn last_post_times_cache_and_compare() {
    let f = fixture();
    let any = PostTypeName::new("any");
    assert_eq!(
        f.api.last_post_date("GMT", &any).unwrap(),
        s("2026-01-03 08:00:00")
    );
    assert_eq!(
        cached(&f, hs("lastpostdate:gmt"), "timeinfo"),
        Some(s("2026-01-03 08:00:00"))
    );
    assert_eq!(
        f.api.last_post_date("gmt", &any).unwrap(),
        s("2026-01-03 08:00:00")
    );
    assert_eq!(calls(&f), ["last_post_time"]);
    // An unknown timezone sends nothing and is false.
    assert_eq!(
        f.api.last_post_date("nope", &any).unwrap(),
        PhpValue::Bool(false)
    );
    assert_eq!(calls(&f), ["last_post_time"]);
    // A cached modified time older than the date is raised to it (last-modified-older-than-date).
    f.cache
        .set(
            &hs("lastpostmodified:gmt"),
            hs("2000-01-01 00:00:00"),
            &group("timeinfo"),
            0,
        )
        .unwrap();
    assert_eq!(
        f.api.last_post_modified("gmt", &any).unwrap(),
        s("2026-01-03 08:00:00")
    );
    // The key carries sanitize_key( $post_type ).
    f.api
        .last_post_date("blog", &PostTypeName::new("Page"))
        .unwrap();
    assert_eq!(
        cached(&f, hs("lastpostdate:blog:page"), "timeinfo"),
        Some(s("2026-01-03 10:00:00"))
    );
}

#[test]
fn handlers_answer_through_the_buses() {
    let f = fixture();
    let container = Rc::new(Container::new());
    register_posts_handlers(&container, f.api.clone());
    assert!(container.has(<GetPost as Query>::KEY));
    assert!(container.has(<WpCountPosts as Query>::KEY));
    assert!(container.has(<CleanPostCache as Command>::KEY));
    assert!(container.has(<PrimePostCaches as Command>::KEY));
    assert!(container.has(<UpdatePostCaches as Command>::KEY));
    assert!(container.has(<UpdatePostParentCaches as Command>::KEY));
    assert_eq!(
        <GetPost as Query>::KEY,
        "SpeedySpec\\WpPosts\\GetPostHandler"
    );
    let bus = speedyspec_cqrs::QueryBus::new(container.clone());
    let read = bus
        .ask(GetPost {
            post: PostRef::Id(PostId(2)),
        })
        .unwrap()
        .unwrap();
    assert_eq!(read.unwrap().id, PostId(2));
}

// ---- fix round 1 ----

/// The `posts` and `post_meta` entries a recorded step's state holds, by id.
fn recorded_group(step: &serde_json::Value, group_name: &str) -> Vec<(PhpKey, PhpValue)> {
    let state = step["state"]["array"].as_array().unwrap();
    let Some(pair) = state
        .iter()
        .find(|pair| pair[0].as_str() == Some(&format!("cache:{group_name}")))
    else {
        return Vec::new();
    };
    let PhpValue::Array(entries) = support::decode(&pair[1]).unwrap() else {
        panic!("a group is an array");
    };
    entries
}

#[test]
fn update_post_parent_caches_replays_recording() {
    let (name, _, steps) = support::scenarios("cache-update-parent-caches")
        .into_iter()
        .next()
        .unwrap();
    // The step after update_post_parent_caches( [ $p3, $p4, $p9 ] ) holds every post involved, and post 1's meta.
    let after = &steps[6];
    let state = after["state"]["array"].as_array().unwrap();
    let posts_pair = state
        .iter()
        .find(|pair| pair[0].as_str() == Some("cache:posts"))
        .unwrap();
    let recorded_posts: Vec<(i64, PhpValue)> = posts_pair[1]["array"]
        .as_array()
        .unwrap()
        .iter()
        .map(|pair| (pair[0].as_i64().unwrap(), support::decode_kept(&pair[1])))
        .collect();
    let rows: Vec<Post> = recorded_posts
        .iter()
        .map(|(_, entry)| PostEntry::from_entry(entry).unwrap())
        .collect();
    let f = fixture_with(rows);
    let post_meta = MetaType::new("post").unwrap();
    for (id, entry) in recorded_group(after, "post_meta") {
        let PhpKey::Int(id) = id else { panic!("an id") };
        let PhpValue::Array(keys) = entry else {
            panic!("a meta entry")
        };
        for (key, values) in keys {
            let PhpKey::Str(key) = key else {
                panic!("a key")
            };
            let PhpValue::Array(values) = values else {
                panic!("values")
            };
            for (_, value) in values {
                f.metadata
                    .add(
                        &post_meta,
                        ObjectId(id as u64),
                        &MetaKey::new(key.clone()),
                        value,
                        false,
                    )
                    .unwrap();
            }
        }
    }

    // get_post( 3 ), ( 4 ), ( 9 ); then the parents 1, 2 and 3 leave the cache.
    let read = |id| {
        f.api
            .get(PostRef::Id(PostId(id)), Filter::Raw)
            .unwrap()
            .unwrap()
    };
    let posts = [read(3), read(4), read(9)];
    for id in [1, 2, 3] {
        f.cache
            .delete(&HookValue::Int(id), &group("posts"))
            .unwrap();
    }
    f.calls.borrow_mut().clear();
    f.api.update_post_parent_caches(&posts).unwrap();
    // One posts statement for the parents (2, 3, 1 in order), and their meta (one update_meta_cache).
    assert_eq!(calls(&f), ["find_many(2,3,1)"], "{name}");
    for (id, entry) in &recorded_posts {
        // The repository's insert, like wp_insert_post(), never writes comment_count, so the seeded rows hold 0.
        let mut entry = entry.clone();
        if let PhpValue::Object { properties, .. } = &mut entry {
            speedyspec_php_serialize::semantics::set(properties, "comment_count", s("0"));
        }
        let entry = &entry;
        assert_eq!(
            cached(&f, HookValue::Int(*id), "posts").as_ref(),
            Some(entry),
            "{name}: post {id}"
        );
    }
    for (id, entry) in recorded_group(after, "post_meta") {
        let PhpKey::Int(id) = id else { panic!("an id") };
        assert_eq!(
            cached(&f, HookValue::Int(id), "post_meta"),
            Some(entry),
            "{name}: meta {id}"
        );
    }
    assert!(
        f.terms.0.borrow().is_empty(),
        "no term cache: _prime_post_caches( $ids, false )"
    );

    // update_post_parent_caches( array() ): nothing.
    f.calls.borrow_mut().clear();
    f.api.update_post_parent_caches(&[]).unwrap();
    assert!(calls(&f).is_empty());
}

#[test]
fn update_post_caches_caches_posts_terms_and_meta() {
    let f = fixture();
    let p1 = post(1, "post", "hello-world", 0, "publish", 1);
    let p2 = post(2, "page", "about", 0, "publish", 1);
    let posts = [p1.clone(), p2.clone(), p1.clone()];
    // A falsy type is 'any': the posts' own types, array_unique()d; every id, repeats kept.
    f.api
        .update_post_caches(&posts, &s(""), true, true)
        .unwrap();
    assert_eq!(*f.terms.0.borrow(), ["update(1,2,1;post,page)"]);
    assert_eq!(
        cached(&f, HookValue::Int(2), "posts"),
        Some(PostEntry::to_entry(&p2))
    );
    assert!(cached(&f, HookValue::Int(1), "post_meta").is_some());
    assert!(cached(&f, HookValue::Int(2), "post_meta").is_some());
    assert!(calls(&f).is_empty(), "the posts are given, not read");

    // A type, or an array of them, is used as given; no terms or meta when not asked; no posts, nothing.
    f.terms.0.borrow_mut().clear();
    f.api
        .update_post_caches(std::slice::from_ref(&p1), &s("page"), true, false)
        .unwrap();
    f.api
        .update_post_caches(
            std::slice::from_ref(&p1),
            &PhpValue::Array(vec![(PhpKey::Int(0), s("a")), (PhpKey::Int(1), s("b"))]),
            true,
            false,
        )
        .unwrap();
    f.api
        .update_post_caches(std::slice::from_ref(&p1), &s("any"), false, false)
        .unwrap();
    f.api
        .update_post_caches(&[], &s("any"), true, true)
        .unwrap();
    assert_eq!(*f.terms.0.borrow(), ["update(1;page)", "update(1;a,b)"]);

    // An entry already cached stays (wp_cache_add_multiple).
    let mut changed = p1.clone();
    changed.title = b"Changed".to_vec();
    f.api
        .update_post_caches(&[changed], &s("post"), false, false)
        .unwrap();
    assert_eq!(
        cached(&f, HookValue::Int(1), "posts"),
        Some(PostEntry::to_entry(&p1))
    );
}

#[test]
fn negative_cached_page_id_is_null_not_the_global() {
    let f = fixture();
    *f.global.0.borrow_mut() = Some(PhpValue::Int(1));
    let page = PostTypeArg::One(PostTypeName::new("page"));
    let salt = f.api.wp_cache_get_last_changed("posts").unwrap();
    let hash: String = <md5::Md5 as md5::Digest>::digest(b"abouts:4:\"page\";")
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let entry = |data: PhpValue| {
        to_hook_value(&PhpValue::Array(vec![
            (PhpKey::Str(b"data".to_vec()), data),
            (PhpKey::Str(b"salt".to_vec()), salt.clone()),
        ]))
    };
    let key = hs(&format!("get_page_by_path:{hash}"));
    // get_post( -1 ) is null: -1 is not empty, and get_instance() rejects it.
    f.cache
        .set(&key, entry(s("-1")), &group("post-queries"), 0)
        .unwrap();
    assert_eq!(f.api.page_by_path("about", &page).unwrap(), None);
    // get_post( (int) 'abc' ) is get_post( 0 ), which reads the global $post, as WordPress does.
    f.cache
        .set(&key, entry(s("abc")), &group("post-queries"), 0)
        .unwrap();
    assert_eq!(
        f.api.page_by_path("about", &page).unwrap().unwrap().id,
        PostId(1)
    );
    assert!(calls(&f).iter().all(|c| c != "pages_by_names"));
}

#[test]
fn entry_with_any_set_id_is_used() {
    let f = fixture();
    // WordPress only asks isset( $_post->ID ): "abc" is used (as ID 0) with no query.
    let entry = PhpValue::Object {
        class: "stdClass".into(),
        properties: vec![
            (PhpKey::Str(b"ID".to_vec()), s("abc")),
            (PhpKey::Str(b"post_title".to_vec()), s("Odd")),
            (PhpKey::Str(b"filter".to_vec()), s("raw")),
        ],
    };
    f.cache
        .set(
            &HookValue::Int(5),
            to_hook_value(&entry),
            &group("posts"),
            0,
        )
        .unwrap();
    let read = f
        .api
        .get(PostRef::Id(PostId(5)), Filter::Raw)
        .unwrap()
        .unwrap();
    assert_eq!(read.title, b"Odd");
    assert_eq!(read.id, PostId(0));
    assert!(calls(&f).is_empty());
    assert_eq!(PostEntry::from_entry(&entry).unwrap().id, PostId(0));
}

#[test]
fn null_property_is_not_a_meta_lookup() {
    let f = fixture();
    let post_meta = MetaType::new("post").unwrap();
    f.metadata
        .add(
            &post_meta,
            ObjectId(1),
            &MetaKey::from("post_title"),
            s("from meta"),
            false,
        )
        .unwrap();
    // A raw object whose post_title is null: the property is accessible, so isset() is false with no __isset().
    *f.global.0.borrow_mut() = Some(PhpValue::Object {
        class: "stdClass".into(),
        properties: vec![
            (PhpKey::Str(b"ID".to_vec()), PhpValue::Int(1)),
            (PhpKey::Str(b"post_title".to_vec()), PhpValue::Null),
            (PhpKey::Str(b"filter".to_vec()), s("raw")),
        ],
    });
    record(&f, "get_post_metadata");
    assert_eq!(
        f.api
            .field("post_title", PostRef::Current, Filter::Raw)
            .unwrap(),
        s("")
    );
    assert!(fired(&f).is_empty());
}

#[test]
fn update_post_cache_objects_key_string_ids_as_ints() {
    let f = fixture();
    let object = PhpValue::Object {
        class: "stdClass".into(),
        properties: vec![
            (PhpKey::Str(b"ID".to_vec()), s("5")),
            (PhpKey::Str(b"post_title".to_vec()), s("Keyed")),
            (PhpKey::Str(b"filter".to_vec()), s("raw")),
        ],
    };
    f.api
        .update_post_cache_objects(vec![object.clone()])
        .unwrap();
    assert_eq!(cached(&f, HookValue::Int(5), "posts"), Some(object));
    assert_eq!(
        f.api
            .get(PostRef::Id(PostId(5)), Filter::Raw)
            .unwrap()
            .unwrap()
            .title,
        b"Keyed"
    );
    assert!(calls(&f).is_empty());
}

/// No option exists (`get_option()` answers false).
struct NoOptions;

impl speedyspec_wp_posts::Options for NoOptions {
    fn get_option(&self, _name: &str) -> Result<PhpValue, PostError> {
        Ok(PhpValue::Bool(false))
    }
}
