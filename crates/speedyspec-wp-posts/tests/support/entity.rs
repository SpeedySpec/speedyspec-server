//! Shared by the entity tests: finds `compat/posts-entity/cases.json`, replays a `dates` case's clock, and builds
//! the slug and hierarchy scenarios' table, the pages' posts, post types, truncation and statement log.

use std::cell::RefCell;
use std::path::PathBuf;

use serde_json::Value;
use speedyspec_php_serialize::semantics::to_php_int;
use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_posts::SlugContext;
use speedyspec_wp_posts_contracts::{
    Clock, CommentStatus, LastPostField, MimeFilter, MimeType, PageRow, PingStatus, Post, PostDate,
    PostDateTime, PostError, PostId, PostParent, PostRepository, PostRow, PostStatusName, PostType,
    PostTypeName, PostTypes, RowValue, Slug, SlugTruncation, Timezone,
};
use speedyspec_wp_posts_memory::MemoryPostRepository;

use super::decode;

/// `post` as the row `wp_insert_post()` writes, with `$data['ID']` set when `id` is given.
fn data_row(post: &Post, id: Option<i64>) -> PostRow {
    let row = PostRow::from_post(post);
    match id {
        Some(id) => row.with("ID", RowValue::Int(id)),
        None => row,
    }
}

/// The recorded entity cases. The file is large: read it once per test.
#[allow(dead_code)]
pub fn entity_cases() -> Value {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../compat/posts-entity/cases.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// A `dates` case's `clock`: the recorded answers of the time functions, and nothing else. It panics when asked
/// for now in a case that recorded `null` (the call should not have read the clock), for a date it was not given,
/// or for a timestamp, which the recording does not hold.
#[allow(dead_code)]
pub struct TableClock {
    now_local: Option<String>,
    now_gmt: Option<String>,
    date_from_gmt: Vec<(Vec<u8>, Vec<u8>)>,
    gmt_from_date: Vec<(Vec<u8>, Vec<u8>)>,
}

#[allow(dead_code)]
impl TableClock {
    /// The clock a case recorded.
    pub fn from_case(clock: &Value) -> Self {
        let now = |name: &str| clock[name].as_str().map(str::to_owned);
        Self {
            now_local: now("now_local"),
            now_gmt: now("now_gmt"),
            date_from_gmt: pairs(&clock["date_from_gmt"]),
            gmt_from_date: pairs(&clock["gmt_from_date"]),
        }
    }

    /// The recorded `get_gmt_from_date()` pairs, `(local, gmt)`.
    pub fn gmt_from_date_pairs(&self) -> &[(Vec<u8>, Vec<u8>)] {
        &self.gmt_from_date
    }
}

/// Decodes `[[in, out]]` pairs of encoded strings.
#[allow(dead_code)]
fn pairs(value: &Value) -> Vec<(Vec<u8>, Vec<u8>)> {
    let bytes = |v: &Value| match decode(v) {
        PhpValue::Str(s) => s,
        other => panic!("a recorded date is a string, not {other:?}"),
    };
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|pair| (bytes(&pair[0]), bytes(&pair[1])))
        .collect()
}

/// The answer recorded for `input`.
#[allow(dead_code)]
fn lookup(table: &[(Vec<u8>, Vec<u8>)], function: &str, input: &[u8]) -> Vec<u8> {
    table
        .iter()
        .find(|(given, _)| given.as_slice() == input)
        .map(|(_, out)| out.clone())
        .unwrap_or_else(|| {
            panic!(
                "{function}({:?}) was not recorded",
                String::from_utf8_lossy(input)
            )
        })
}

impl Clock for TableClock {
    fn now_local(&self) -> String {
        self.now_local
            .clone()
            .expect("current_time( 'mysql' ) read in a case that recorded no now")
    }

    fn now_gmt(&self) -> String {
        self.now_gmt
            .clone()
            .expect("current_time( 'mysql', true ) read in a case that recorded no now")
    }

    fn date_from_gmt(&self, gmt: &[u8]) -> Vec<u8> {
        lookup(&self.date_from_gmt, "get_date_from_gmt", gmt)
    }

    fn gmt_from_date(&self, local: &[u8]) -> Vec<u8> {
        lookup(&self.gmt_from_date, "get_gmt_from_date", local)
    }

    fn timestamp(&self, datetime: &[u8]) -> Option<i64> {
        panic!(
            "strtotime({:?}) is not recorded",
            String::from_utf8_lossy(datetime)
        )
    }
}

/// The tests' `SlugTruncation`: the text seam's `wp_truncate_slug()`.
#[allow(dead_code)]
pub struct TextTruncation;

impl SlugTruncation for TextTruncation {
    fn truncate(&self, slug: &[u8], length: usize) -> Vec<u8> {
        speedyspec_wp_text::helpers::wp_truncate_slug(
            slug,
            i64::try_from(length).unwrap_or(i64::MAX),
        )
    }
}

/// A scenario's `post_types`: each a name and whether it is hierarchical.
#[allow(dead_code)]
pub struct FakeTypes(Vec<PostType>);

#[allow(dead_code)]
impl FakeTypes {
    /// The post types a scenario registered, `[[name, hierarchical]]`.
    pub fn from_scenario(post_types: &Value) -> Self {
        Self(
            post_types
                .as_array()
                .unwrap()
                .iter()
                .map(|pair| fake_type(pair[0].as_str().unwrap(), pair[1].as_bool().unwrap()))
                .collect(),
        )
    }

    /// These post types.
    pub fn new(types: &[(&str, bool)]) -> Self {
        Self(types.iter().map(|(n, h)| fake_type(n, *h)).collect())
    }
}

/// A post type with a `name` and `hierarchical`.
#[allow(dead_code)]
fn fake_type(name: &str, hierarchical: bool) -> PostType {
    PostType::new(
        PostTypeName::new(name),
        vec![
            (
                PhpKey::Str(b"name".to_vec()),
                PhpValue::Str(name.as_bytes().to_vec()),
            ),
            (
                PhpKey::Str(b"hierarchical".to_vec()),
                PhpValue::Bool(hierarchical),
            ),
        ],
    )
}

impl PostTypes for FakeTypes {
    fn post_type(&self, name: &str) -> Option<PostType> {
        self.0.iter().find(|t| t.name().as_str() == name).cloned()
    }

    fn post_types(&self) -> Vec<PostType> {
        self.0.clone()
    }
}

/// A recorded row's columns (all strings, as `$wpdb` returns them) as a [`Post`]; also a recorded `WP_Post`, whose
/// `ID`, `post_parent` and `menu_order` `get_instance()` cast to int.
#[allow(dead_code)]
pub fn row_post(row: &Value) -> Post {
    let columns = match super::decode(row) {
        PhpValue::Array(columns) => columns,
        PhpValue::Object { class, properties } if class == "WP_Post" => properties,
        other => panic!("a recorded row is an array or a WP_Post, not {other:?}"),
    };
    let column = |name: &str| -> Vec<u8> {
        columns
            .iter()
            .find(|(k, _)| matches!(k, PhpKey::Str(key) if key == name.as_bytes()))
            .map(|(_, v)| match v {
                PhpValue::Str(s) => s.clone(),
                PhpValue::Int(n) => n.to_string().into_bytes(),
                other => panic!("column {name} is a string or an int, not {other:?}"),
            })
            .unwrap_or_else(|| panic!("the row has no {name}"))
    };
    let text = |name: &str| String::from_utf8_lossy(&column(name)).into_owned();
    let int = |name: &str| to_php_int(&PhpValue::Str(column(name)));
    let id = |name: &str| PostId(u64::try_from(int(name)).unwrap());
    let date = |local: &str, gmt: &str| PostDate {
        local: PostDateTime::new(text(local)),
        gmt: PostDateTime::new(text(gmt)),
    };
    Post {
        id: id("ID"),
        author: u64::try_from(int("post_author")).unwrap(),
        date: date("post_date", "post_date_gmt"),
        content: column("post_content"),
        title: column("post_title"),
        excerpt: column("post_excerpt"),
        status: PostStatusName::new(text("post_status")),
        comment_status: CommentStatus::new(text("comment_status")),
        ping_status: PingStatus::new(text("ping_status")),
        password: column("post_password"),
        name: Slug::new(column("post_name")),
        to_ping: column("to_ping"),
        pinged: column("pinged"),
        modified: date("post_modified", "post_modified_gmt"),
        content_filtered: column("post_content_filtered"),
        parent: PostParent::from(id("post_parent")),
        guid: column("guid"),
        menu_order: int("menu_order"),
        post_type: PostTypeName::new(text("post_type")),
        mime_type: MimeType::new(column("post_mime_type")),
        comment_count: int("comment_count"),
    }
}

/// A memory table holding a scenario's `rows`, each inserted with its ID.
#[allow(dead_code)]
pub fn seed(rows: &Value) -> MemoryPostRepository {
    let repository = MemoryPostRepository::new();
    for row in rows.as_array().unwrap() {
        let post = row_post(row);
        let id = post.id;
        assert_eq!(
            repository
                .insert(&data_row(&post, Some(id.0 as i64)))
                .unwrap(),
            Some(id),
            "seeding {id:?}"
        );
    }
    repository
}

/// One logged repository call: the recorder's label and its arguments.
#[allow(dead_code)]
pub type Statement = (String, Vec<PhpValue>);

/// A [`PostRepository`] decorator (Decorator) logging each read as the recorder logs `$wpdb` statements:
/// `find`, `attachment_slug`, `hierarchical_slug` and `flat_slug`, with their arguments.
#[allow(dead_code)]
pub struct Recording<R> {
    inner: R,
    log: RefCell<Vec<Statement>>,
}

#[allow(dead_code)]
impl<R: PostRepository> Recording<R> {
    /// Logs reads of `inner`.
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            log: RefCell::new(Vec::new()),
        }
    }

    /// The statements logged since the last take, which clears them.
    pub fn take(&self) -> Vec<Statement> {
        std::mem::take(&mut self.log.borrow_mut())
    }

    fn record(&self, label: &str, args: Vec<PhpValue>) {
        self.log.borrow_mut().push((label.to_owned(), args));
    }
}

/// A [`PostId`] as the recorder's `%d` argument.
#[allow(dead_code)]
fn id_arg(id: PostId) -> PhpValue {
    PhpValue::Int(i64::try_from(id.0).unwrap())
}

impl<R: PostRepository> PostRepository for Recording<R> {
    fn engine(&self) -> String {
        self.inner.engine()
    }

    fn find(&self, id: PostId) -> Result<Option<Post>, PostError> {
        self.record("find", vec![id_arg(id)]);
        self.inner.find(id)
    }

    fn insert(&self, row: &PostRow) -> Result<Option<PostId>, PostError> {
        self.inner.insert(row)
    }

    fn attachment_slug_owner(
        &self,
        slug: &Slug,
        except: PostId,
    ) -> Result<Option<Slug>, PostError> {
        self.record(
            "attachment_slug",
            vec![PhpValue::Str(slug.as_bytes().to_vec()), id_arg(except)],
        );
        self.inner.attachment_slug_owner(slug, except)
    }

    fn hierarchical_slug_owner(
        &self,
        slug: &Slug,
        post_type: &PostTypeName,
        except: PostId,
        parent: PostParent,
    ) -> Result<Option<Slug>, PostError> {
        self.record(
            "hierarchical_slug",
            vec![
                PhpValue::Str(slug.as_bytes().to_vec()),
                PhpValue::Str(post_type.as_str().as_bytes().to_vec()),
                id_arg(except),
                PhpValue::Int(parent.prepared()),
            ],
        );
        self.inner
            .hierarchical_slug_owner(slug, post_type, except, parent)
    }

    fn flat_slug_owner(
        &self,
        slug: &Slug,
        post_type: &PostTypeName,
        except: PostId,
    ) -> Result<Option<Slug>, PostError> {
        self.record(
            "flat_slug",
            vec![
                PhpValue::Str(slug.as_bytes().to_vec()),
                PhpValue::Str(post_type.as_str().as_bytes().to_vec()),
                id_arg(except),
            ],
        );
        self.inner.flat_slug_owner(slug, post_type, except)
    }

    fn find_many(&self, ids: &[PostId]) -> Result<Vec<Post>, PostError> {
        self.inner.find_many(ids)
    }

    fn parent_ids(&self, ids: &[PostId]) -> Result<Vec<(PostId, PostId)>, PostError> {
        self.inner.parent_ids(ids)
    }

    fn pages_by_names(
        &self,
        names: &[Slug],
        post_types: &[PostTypeName],
    ) -> Result<Vec<PageRow>, PostError> {
        self.inner.pages_by_names(names, post_types)
    }

    fn all_page_ids(&self) -> Result<Vec<PostId>, PostError> {
        self.inner.all_page_ids()
    }

    fn count_by_status(
        &self,
        post_type: &PostTypeName,
        reader: Option<u64>,
    ) -> Result<Vec<(PostStatusName, u64)>, PostError> {
        self.inner.count_by_status(post_type, reader)
    }

    fn count_attachments(
        &self,
        mime_type: Option<&MimeFilter>,
    ) -> Result<(Vec<(MimeType, u64)>, u64), PostError> {
        self.inner.count_attachments(mime_type)
    }

    fn last_post_time(
        &self,
        field: LastPostField,
        post_types: &[PostTypeName],
        timezone: Timezone,
    ) -> Result<Option<PostDateTime>, PostError> {
        self.inner.last_post_time(field, post_types, timezone)
    }

    fn available_mime_types(&self, post_type: &PostTypeName) -> Result<Vec<MimeType>, PostError> {
        self.inner.available_mime_types(post_type)
    }

    fn update(&self, id: PostId, row: &PostRow) -> Result<Option<u64>, PostError> {
        self.inner.update(id, row)
    }

    fn update_name(&self, id: PostId, name: &Slug) -> Result<Option<u64>, PostError> {
        self.inner.update_name(id, name)
    }

    fn update_guid(&self, id: PostId, guid: &[u8]) -> Result<Option<u64>, PostError> {
        self.inner.update_guid(id, guid)
    }

    fn update_status(&self, id: PostId, status: &PostStatusName) -> Result<Option<u64>, PostError> {
        self.inner.update_status(id, status)
    }

    fn update_pinged(&self, id: PostId, pinged: &[u8]) -> Result<Option<u64>, PostError> {
        self.inner.update_pinged(id, pinged)
    }

    fn update_post_type(
        &self,
        id: i64,
        post_type: &PostTypeName,
    ) -> Result<Option<u64>, PostError> {
        self.inner.update_post_type(id, post_type)
    }

    fn import_id_taken(&self, id: i64) -> Result<bool, PostError> {
        self.inner.import_id_taken(id)
    }

    fn raw_row(&self, id: PostId) -> Result<Option<Post>, PostError> {
        self.inner.raw_row(id)
    }

    fn children_of_type(
        &self,
        parent: PostId,
        post_type: &PostTypeName,
    ) -> Result<Vec<Post>, PostError> {
        self.inner.children_of_type(parent, post_type)
    }

    fn reparent_children(
        &self,
        from: PostId,
        to: PostParent,
        post_type: &PostTypeName,
    ) -> Result<Option<u64>, PostError> {
        self.inner.reparent_children(from, to, post_type)
    }

    fn revision_ids(&self, parent: PostId) -> Result<Vec<PostId>, PostError> {
        self.inner.revision_ids(parent)
    }

    fn reparent_attachments(&self, from: PostId, to: PostParent) -> Result<Option<u64>, PostError> {
        self.inner.reparent_attachments(from, to)
    }

    fn remove(&self, id: PostId) -> Result<Option<u64>, PostError> {
        self.inner.remove(id)
    }

    fn auto_drafts_older_than_a_week(&self) -> Result<Vec<PostId>, PostError> {
        self.inner.auto_drafts_older_than_a_week()
    }
}

/// A recording's `statements`, decoded.
#[allow(dead_code)]
pub fn statements(recorded: &Value) -> Vec<Statement> {
    recorded
        .as_array()
        .unwrap()
        .iter()
        .map(|statement| {
            let args = statement[1]
                .as_array()
                .unwrap()
                .iter()
                .map(super::decode)
                .collect();
            (statement[0].as_str().unwrap().to_owned(), args)
        })
        .collect()
}

/// WordPress's default rewrite feeds and pagination base, with a scenario's permalink structure.
#[allow(dead_code)]
pub fn context(permalinks: &str) -> SlugContext {
    SlugContext {
        feeds: ["feed", "rdf", "rss", "rss2", "atom"]
            .iter()
            .map(|f| f.as_bytes().to_vec())
            .collect(),
        pagination_base: b"page".to_vec(),
        permalink_structure: permalinks.as_bytes().to_vec(),
    }
}
