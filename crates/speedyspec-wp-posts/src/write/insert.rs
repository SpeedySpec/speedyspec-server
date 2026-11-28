//! `wp_insert_post( $postarr, $wp_error, $fire_after_hooks )` (post.php 4839-5543, WordPress 7.2-RC2), as a
//! Template Method: [`Insert::run`] calls the steps in WordPress's order, and each step is a pure function or a call
//! through a port.
//!
//! The order is WordPress's exactly (spec, *Post write path*; the phase 2b ruling):
//!
//! 1. [`Insert::parse`]: the defaults, `sanitize_post( 'db' )`, and the update case (`get_post()` of the id).
//! 2. [`Insert::check_content`]: the `wp_insert_post_empty_content` filter.
//! 3. [`Insert::resolve`]: status, categories, the `pending` slug rule, `sanitize_title()`, `wp_resolve_post_date()`
//!    and the dates ([`insert_post_dates`]), comment and ping status, password, menu order and parent.
//! 4. [`Insert::parent`]: the `wp_insert_post_parent` filter, whose default callback
//!    (`wp_check_post_hierarchy_for_loops()`, [`crate::write::transition`]) breaks a loop through `wp_update_post()`.
//! 5. [`Insert::slug`]: the trashed-slug suffixes, then `wp_unique_post_slug()` (Strategy: hierarchical or flat).
//! 6. [`Insert::data`]: `$data`, the emoji check, the `wp_insert_post_data` filter and `wp_unslash()`.
//! 7. [`Insert::write`]: `pre_post_update` and the repository update, or the `import_id` check, `pre_post_insert` and
//!    the repository insert.
//! 8. [`Insert::after_write`]: the slug of a post that had none, terms, meta input, GUID, the attachment's file and
//!    context, the featured image, and `clean_post_cache()`.
//! 9. [`Insert::finish`]: `get_post()`, the page template, the transition, `edit_post`, `post_updated`, the
//!    `save_post` family and `wp_after_insert_post`.
//!
//! After every write that answers empty or 0, [`hidden_failure`] checks `wpdb`'s side channel. A port's failure stops
//! the use case where it happens; whatever WordPress wrote before stays written.

use std::ops::ControlFlow;

use speedyspec_php_serialize::semantics::{
    is_empty, isset, key_to_string, set, to_array, to_php_int, to_php_string, unset,
};
use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_hooks_contracts::HookValue;
use speedyspec_wp_posts_contracts::{
    CommentStatus, MimeType, PingStatus, Post, PostDate, PostDateTime, PostError, PostId,
    PostObject, PostParent, PostRef, PostStatusName, PostTypeName, PostWriteError, Slug,
    WriteResult,
};

use crate::hook::hv;
use crate::read::Loaded;
use crate::write::api::{PostWritesApi, SlugReader, checked_rows, hidden_failure, int, post_id};
use crate::write::php::{
    Items, array_filter, at, empty_at, is_str, key, or, s, wp_parse_args, wp_unslash,
};
use crate::write::ports::HookBadSlugs;
use crate::{
    InsertDateInput, ResolvedPostDate, SlugPorts, SlugRequest, insert_post_dates,
    resolve_post_date, sanitize_post_value, unique_post_slug_filtered,
};

/// A step's answer: go on with a value, or return what WordPress returns there.
type Flow<T> = Result<ControlFlow<WriteResult, T>, PostError>;

/// Unwraps a [`Flow`] inside a function returning one, a `WriteResult` or a `Result<WriteResult, _>`.
macro_rules! step {
    ($e:expr) => {
        match $e? {
            ControlFlow::Continue(v) => v,
            ControlFlow::Break(answer) => return Ok(ControlFlow::Break(answer)),
        }
    };
}

/// The statuses that keep an empty slug (post.php 4994, 5291).
const DRAFTS: [&str; 3] = ["draft", "pending", "auto-draft"];

/// `$data`'s keys, in `compact()`'s order (post.php 5164-5186).
const DATA_KEYS: [&str; 21] = [
    "post_author",
    "post_date",
    "post_date_gmt",
    "post_content",
    "post_content_filtered",
    "post_title",
    "post_excerpt",
    "post_status",
    "post_type",
    "comment_status",
    "ping_status",
    "post_password",
    "post_name",
    "to_ping",
    "pinged",
    "post_modified",
    "post_modified_gmt",
    "post_parent",
    "menu_order",
    "post_mime_type",
    "guid",
];

/// The text of a PHP value, as `(string)` gives it (lossily, for a name that must be UTF-8).
fn text(value: &PhpValue) -> String {
    String::from_utf8_lossy(&to_php_string(value)).into_owned()
}

/// A post id as a hook argument.
fn id_arg(id: PostId) -> HookValue {
    HookValue::Int(int(id))
}

/// `wp_insert_post()`'s defaults (post.php 4847-4867): `post_author` is the current user.
fn defaults(user_id: u64) -> Items {
    let int = |n: u64| PhpValue::Int(i64::try_from(n).unwrap_or(i64::MAX));
    [
        ("post_author", int(user_id)),
        ("post_content", s("")),
        ("post_content_filtered", s("")),
        ("post_title", s("")),
        ("post_excerpt", s("")),
        ("post_status", s("draft")),
        ("post_type", s("post")),
        ("comment_status", s("")),
        ("ping_status", s("")),
        ("post_password", s("")),
        ("to_ping", s("")),
        ("pinged", s("")),
        ("post_parent", PhpValue::Int(0)),
        ("menu_order", PhpValue::Int(0)),
        ("guid", s("")),
        ("import_id", PhpValue::Int(0)),
        ("context", s("")),
        ("post_date", s("")),
        ("post_date_gmt", s("")),
    ]
    .into_iter()
    .map(|(k, v)| (key(k), v))
    .collect()
}

/// Data Mapper: `$data` after `wp_unslash()` as the [`Post`] entity the repository writes, `id` its `ID`.
///
/// Each column is read as `$wpdb->insert` and `$wpdb->update` print it: `%d` columns with `(int)`, the rest with
/// `(string)`. A known divergence: a column a `wp_insert_post_data` callback removed from `$data` is written with its
/// default here, where `$wpdb` would leave it out of the statement.
fn post_from_data(data: &Items, id: PostId) -> Post {
    let bytes = |name: &str| at(data, name).map(to_php_string).unwrap_or_default();
    let string = |name: &str| String::from_utf8_lossy(&bytes(name)).into_owned();
    let date = |name: &str| PostDateTime::new(string(name));
    Post {
        id,
        author: u64::try_from(at(data, "post_author").map_or(0, to_php_int)).unwrap_or(0),
        date: PostDate {
            local: date("post_date"),
            gmt: date("post_date_gmt"),
        },
        content: bytes("post_content"),
        title: bytes("post_title"),
        excerpt: bytes("post_excerpt"),
        status: PostStatusName::new(string("post_status")),
        comment_status: CommentStatus::new(string("comment_status")),
        ping_status: PingStatus::new(string("ping_status")),
        password: bytes("post_password"),
        name: Slug::new(bytes("post_name")),
        to_ping: bytes("to_ping"),
        pinged: bytes("pinged"),
        modified: PostDate {
            local: date("post_modified"),
            gmt: date("post_modified_gmt"),
        },
        content_filtered: bytes("post_content_filtered"),
        parent: match at(data, "post_parent") {
            Some(PhpValue::Null) => PostParent::Null,
            Some(v) => PostParent::Int(to_php_int(v)),
            None => PostParent::NONE,
        },
        guid: bytes("guid"),
        menu_order: at(data, "menu_order").map_or(0, to_php_int),
        post_type: PostTypeName::new(string("post_type")),
        mime_type: MimeType::new(bytes("post_mime_type")),
        comment_count: 0,
    }
}

/// `wp_insert_post()`'s variables, filled step by step.
pub(crate) struct Insert<'a> {
    api: &'a PostWritesApi,
    wp_error: bool,
    fire_after_hooks: bool,
    /// `$unsanitized_postarr`.
    unsanitized: PhpValue,
    /// `$postarr`.
    postarr: Items,
    user_id: u64,
    update: bool,
    /// `$post_id`.
    post_id: PostId,
    /// `$post_before`, and its object once a hook has received it.
    post_before: Option<Loaded>,
    post_before_handle: Option<HookValue>,
    guid: PhpValue,
    previous_status: String,
    post_type: String,
    post_title: PhpValue,
    post_content: PhpValue,
    post_excerpt: PhpValue,
    /// `$post_name`; `None` while PHP has it undefined.
    post_name: Option<PhpValue>,
    post_status: String,
    post_category: PhpValue,
    post_date: Vec<u8>,
    post_date_gmt: Vec<u8>,
    post_modified: Vec<u8>,
    post_modified_gmt: Vec<u8>,
    comment_status: PhpValue,
    ping_status: PhpValue,
    post_content_filtered: PhpValue,
    post_author: PhpValue,
    to_ping: PhpValue,
    pinged: PhpValue,
    import_id: PhpValue,
    menu_order: i64,
    post_password: PhpValue,
    post_parent: PostParent,
    data: Items,
}

impl<'a> Insert<'a> {
    pub(crate) fn new(
        api: &'a PostWritesApi,
        postarr: PhpValue,
        wp_error: bool,
        fire_after_hooks: bool,
    ) -> Self {
        Self {
            api,
            wp_error,
            fire_after_hooks,
            unsanitized: postarr,
            postarr: Vec::new(),
            user_id: 0,
            update: false,
            post_id: PostId(0),
            post_before: None,
            post_before_handle: None,
            guid: s(""),
            previous_status: "new".to_owned(),
            post_type: "post".to_owned(),
            post_title: s(""),
            post_content: s(""),
            post_excerpt: s(""),
            post_name: None,
            post_status: "draft".to_owned(),
            post_category: PhpValue::Array(Vec::new()),
            post_date: Vec::new(),
            post_date_gmt: Vec::new(),
            post_modified: Vec::new(),
            post_modified_gmt: Vec::new(),
            comment_status: s(""),
            ping_status: s(""),
            post_content_filtered: s(""),
            post_author: PhpValue::Int(0),
            to_ping: s(""),
            pinged: s(""),
            import_id: PhpValue::Int(0),
            menu_order: 0,
            post_password: s(""),
            post_parent: PostParent::NONE,
            data: Vec::new(),
        }
    }

    /// The Template Method: every step, in WordPress's order.
    pub(crate) fn run(mut self) -> Result<WriteResult, PostError> {
        Ok(match self.steps()? {
            ControlFlow::Continue(answer) | ControlFlow::Break(answer) => answer,
        })
    }

    fn steps(&mut self) -> Flow<WriteResult> {
        step!(self.parse());
        step!(self.check_content());
        step!(self.resolve());
        self.parent()?;
        self.slug()?;
        self.data()?;
        step!(self.write());
        self.after_write()?;
        self.finish()
    }

    fn refuse<T>(&self, error: PostWriteError) -> Flow<T> {
        Ok(ControlFlow::Break(PostWritesApi::refuse(
            self.wp_error,
            error,
        )))
    }

    /// Step 1 (post.php 4843-4903): the defaults, `wp_parse_args()`, `sanitize_post( 'db' )`, and the update case.
    fn parse(&mut self) -> Flow<()> {
        let api = self.api;
        self.user_id = api.d.users.get_current_user_id()?;
        let postarr = std::mem::replace(&mut self.unsanitized, PhpValue::Null);
        self.unsanitized = postarr.clone();
        let mut parsed = wp_parse_args(postarr, defaults(self.user_id), |parsed| {
            api.filter("wp_parse_str", parsed, Vec::new)
        })?;
        unset(&mut parsed, "filter");
        let sanitized =
            sanitize_post_value(&api.d.posts.sanitize_ports(), PhpValue::Array(parsed), "db")?;
        self.postarr = to_array(sanitized);
        self.guid = or(&self.postarr, "guid", PhpValue::Null);
        if !empty_at(&self.postarr, "ID") {
            self.update = true;
            let id = at(&self.postarr, "ID").map_or(0, to_php_int);
            self.post_id = post_id(id);
            let before = if id > 0 {
                api.d.posts.instance(id)?
            } else {
                None
            };
            let Some(before) = before else {
                return self.refuse(PostWriteError::InvalidPost);
            };
            self.post_before = Some(before);
            self.guid = api.post_field("guid", self.post_id)?;
            self.previous_status = text(&api.post_field("post_status", self.post_id)?);
        }
        let post_type = at(&self.postarr, "post_type")
            .cloned()
            .unwrap_or(PhpValue::Null);
        self.post_type = if is_empty(&post_type) {
            "post".to_owned()
        } else {
            text(&post_type)
        };
        self.post_title = or(&self.postarr, "post_title", PhpValue::Null);
        self.post_content = or(&self.postarr, "post_content", PhpValue::Null);
        self.post_excerpt = or(&self.postarr, "post_excerpt", PhpValue::Null);
        self.post_name = match isset(&self.postarr, "post_name") {
            Some(name) => Some(name.clone()),
            None => self
                .post_before
                .as_ref()
                .map(|before| s(before.post.name.as_bytes())),
        };
        Ok(ControlFlow::Continue(()))
    }

    /// Step 2 (post.php 4925-4948): `$maybe_empty`, then `wp_insert_post_empty_content`.
    fn check_content(&mut self) -> Flow<()> {
        let supports = |feature: &str| self.api.d.supports.supports(&self.post_type, feature);
        let maybe_empty = "attachment" != self.post_type
            && is_empty(&self.post_content)
            && is_empty(&self.post_title)
            && is_empty(&self.post_excerpt)
            && supports("editor")
            && supports("title")
            && supports("excerpt");
        let postarr = PhpValue::Array(self.postarr.clone());
        let empty = self.api.filter(
            "wp_insert_post_empty_content",
            PhpValue::Bool(maybe_empty),
            || vec![hv(&postarr)],
        )?;
        if !is_empty(&empty) {
            return self.refuse(PostWriteError::EmptyContent);
        }
        Ok(ControlFlow::Continue(()))
    }

    /// Step 3 (post.php 4950-5104): status, categories, the slug, the dates, comment and ping status, author, pings,
    /// menu order, password and the parent before its filter.
    fn resolve(&mut self) -> Flow<()> {
        let api = self.api;
        let status = at(&self.postarr, "post_status")
            .cloned()
            .unwrap_or(PhpValue::Null);
        self.post_status = if is_empty(&status) {
            "draft".to_owned()
        } else {
            text(&status)
        };
        if "attachment" == self.post_type
            && !matches!(
                self.post_status.as_str(),
                "inherit" | "private" | "trash" | "auto-draft"
            )
        {
            self.post_status = "inherit".to_owned();
        }

        self.categories()?;
        self.pending_slug()?;
        self.sanitize_slug()?;

        // wp_resolve_post_date(), then the dates and the publish/future swap.
        let date = or(&self.postarr, "post_date", PhpValue::Null);
        let date_gmt = or(&self.postarr, "post_date_gmt", PhpValue::Null);
        let Some(post_date) =
            self.resolve_date(&to_php_string(&date), &to_php_string(&date_gmt))?
        else {
            return self.refuse(PostWriteError::InvalidDate);
        };
        let given_gmt = to_php_string(&date_gmt);
        let dates = insert_post_dates(
            &InsertDateInput {
                post_date: &post_date,
                post_date_gmt: &given_gmt,
                status: &self.post_status,
                post_type: &self.post_type,
                update: self.update,
            },
            &*api.d.posts.deps().statuses,
            &*api.d.clock,
        );
        self.post_date = post_date;
        self.post_date_gmt = dates.date_gmt;
        self.post_modified = dates.modified;
        self.post_modified_gmt = dates.modified_gmt;
        self.post_status = dates.status;

        let post_type = PostTypeName::new(self.post_type.clone());
        self.comment_status = if empty_at(&self.postarr, "comment_status") {
            if self.update {
                s("closed")
            } else {
                api.d
                    .comments
                    .default_comment_status(&post_type, "comment")?
            }
        } else {
            or(&self.postarr, "comment_status", PhpValue::Null)
        };
        self.post_content_filtered = or(&self.postarr, "post_content_filtered", PhpValue::Null);
        self.post_author = or(
            &self.postarr,
            "post_author",
            PhpValue::Int(i64::try_from(self.user_id).unwrap_or(i64::MAX)),
        );
        self.ping_status = if empty_at(&self.postarr, "ping_status") {
            api.d
                .comments
                .default_comment_status(&post_type, "pingback")?
        } else {
            or(&self.postarr, "ping_status", PhpValue::Null)
        };
        self.to_ping = match isset(&self.postarr, "to_ping") {
            Some(to_ping) => api
                .d
                .posts
                .deps()
                .formatting
                .sanitize_trackback_urls(to_ping.clone())?,
            None => s(""),
        };
        self.pinged = or(&self.postarr, "pinged", s(""));
        self.import_id = or(&self.postarr, "import_id", PhpValue::Int(0));
        self.menu_order = isset(&self.postarr, "menu_order").map_or(0, to_php_int);
        self.post_password = or(&self.postarr, "post_password", s(""));
        if "private" == self.post_status {
            self.post_password = s("");
        }
        self.post_parent =
            PostParent::Int(isset(&self.postarr, "post_parent").map_or(0, to_php_int));
        Ok(ControlFlow::Continue(()))
    }

    /// `$post_category` (post.php 4961-4977): the given categories without empty ones, the post's own on an update
    /// that names none, else the default category for a `post` that is not an auto-draft.
    fn categories(&mut self) -> Result<(), PostError> {
        let api = self.api;
        let mut category = None;
        if !empty_at(&self.postarr, "post_category") {
            category = Some(array_filter(
                at(&self.postarr, "post_category")
                    .cloned()
                    .unwrap_or(PhpValue::Null),
            ));
        } else if self.update && isset(&self.postarr, "post_category").is_none() {
            let before = self.post_before.as_ref().expect("an update has a post");
            category = Some(api.d.taxonomy.post_term_fields(
                &before.object(),
                "category",
                "term_id",
            )?);
        }
        let usable = category
            .as_ref()
            .is_some_and(|c| matches!(c, PhpValue::Array(items) if !items.is_empty()));
        self.post_category = if usable {
            category.expect("checked")
        } else if "post" == self.post_type && "auto-draft" != self.post_status {
            PhpValue::Array(vec![(PhpKey::Int(0), api.option("default_category")?)])
        } else {
            PhpValue::Array(Vec::new())
        };
        Ok(())
    }

    /// A user who cannot publish leaves a `pending` post's slug empty (post.php 4983-4991).
    fn pending_slug(&mut self) -> Result<(), PostError> {
        if "pending" != self.post_status {
            return Ok(());
        }
        let api = self.api;
        let type_object = api.d.posts.deps().types.post_type(&self.post_type);
        if !self.update
            && let Some(type_object) = type_object
        {
            let cap = type_object
                .get("cap")
                .and_then(|cap| crate::write::php::object_property(cap, "publish_posts"))
                .unwrap_or(PhpValue::Null);
            if !api.d.users.current_user_can(&cap, &[])? {
                self.post_name = Some(s(""));
            }
        } else if self.update
            && !api
                .d
                .users
                .current_user_can(&s("publish_post"), &[PhpValue::Int(int(self.post_id))])?
        {
            self.post_name = Some(s(""));
        }
        Ok(())
    }

    /// `$post_name` through `sanitize_title()` (post.php 4993-5014): a draft keeps an empty slug, a published post
    /// takes its title's; an update keeps a slug from the old `old-save` sanitizing when it is the stored one.
    fn sanitize_slug(&mut self) -> Result<(), PostError> {
        let api = self.api;
        let fmt = &*api.d.formatting;
        let name = self.post_name.clone().unwrap_or(PhpValue::Null);
        self.post_name = Some(if is_empty(&name) {
            if DRAFTS.contains(&self.post_status.as_str()) {
                s("")
            } else {
                fmt.sanitize_title(&self.post_title, &s(""), "save")?
            }
        } else {
            let check = fmt.sanitize_title(&name, &s(""), "old-save")?;
            let keeps = self.update
                && crate::write::php::identical(
                    &s(urlencode(&to_php_string(&name)).to_ascii_lowercase()),
                    &check,
                )
                && crate::write::php::identical(
                    &api.post_field("post_name", self.post_id)?,
                    &check,
                );
            if keeps {
                check
            } else {
                fmt.sanitize_title(&name, &s(""), "save")?
            }
        });
        Ok(())
    }

    /// `wp_resolve_post_date( $post_date, $post_date_gmt )` with its `wp_checkdate` filter (post.php 5761-5784).
    fn resolve_date(&self, date: &[u8], date_gmt: &[u8]) -> Result<Option<Vec<u8>>, PostError> {
        match resolve_post_date(date, date_gmt, &*self.api.d.clock) {
            ResolvedPostDate::NoDate => Ok(None),
            ResolvedPostDate::Checked { date, valid } => {
                let source = s(&date);
                let valid = self
                    .api
                    .filter("wp_checkdate", PhpValue::Bool(valid), || vec![hv(&source)])?;
                Ok((!is_empty(&valid)).then_some(date))
            }
        }
    }

    /// `$new_postarr` (post.php 5106-5111): `ID`, then `compact()` of the defaults' keys but `context` and `filter`.
    fn new_postarr(&self) -> Items {
        let date = |b: &[u8]| s(b);
        let mut items: Items = vec![(key("ID"), PhpValue::Int(int(self.post_id)))];
        let parent = match self.post_parent {
            PostParent::Int(n) => PhpValue::Int(n),
            PostParent::Null => PhpValue::Null,
        };
        for (name, value) in [
            ("post_author", self.post_author.clone()),
            ("post_content", self.post_content.clone()),
            ("post_content_filtered", self.post_content_filtered.clone()),
            ("post_title", self.post_title.clone()),
            ("post_excerpt", self.post_excerpt.clone()),
            ("post_status", s(&self.post_status)),
            ("post_type", s(&self.post_type)),
            ("comment_status", self.comment_status.clone()),
            ("ping_status", self.ping_status.clone()),
            ("post_password", self.post_password.clone()),
            ("to_ping", self.to_ping.clone()),
            ("pinged", self.pinged.clone()),
            ("post_parent", parent),
            ("menu_order", PhpValue::Int(self.menu_order)),
            ("guid", self.guid.clone()),
            ("import_id", self.import_id.clone()),
            ("post_date", date(&self.post_date)),
            ("post_date_gmt", date(&self.post_date_gmt)),
        ] {
            items.push((key(name), value));
        }
        items
    }

    /// Step 4 (post.php 5113-5124): the `wp_insert_post_parent` filter. Its default callback,
    /// `wp_check_post_hierarchy_for_loops()`, runs inside it and breaks a loop through `wp_update_post()` before any
    /// slug is chosen. A known divergence: an answer that is not an int is cast with `(int)` (`null` stays null).
    fn parent(&mut self) -> Result<(), PostError> {
        let parent = match self.post_parent {
            PostParent::Int(n) => PhpValue::Int(n),
            PostParent::Null => PhpValue::Null,
        };
        let new_postarr = PhpValue::Array(self.new_postarr());
        let postarr = PhpValue::Array(self.postarr.clone());
        let id = self.post_id;
        let answer = self.api.filter("wp_insert_post_parent", parent, || {
            vec![id_arg(id), hv(&new_postarr), hv(&postarr)]
        })?;
        self.post_parent = match answer {
            PhpValue::Null => PostParent::Null,
            other => PostParent::Int(to_php_int(&other)),
        };
        Ok(())
    }

    /// Step 5 (post.php 5126-5160): a restored post takes back its desired slug, a trashed post holding the wanted
    /// slug gives it up, a post being trashed takes the `__trashed` suffix, then `wp_unique_post_slug()`.
    fn slug(&mut self) -> Result<(), PostError> {
        let api = self.api;
        let id = self.post_id;
        if "trash" == self.previous_status && "trash" != self.post_status {
            let desired = api.get_meta(id, b"_wp_desired_post_slug", true)?;
            if !is_empty(&desired) {
                api.delete_meta(id, b"_wp_desired_post_slug")?;
                self.post_name = Some(desired);
            }
        }
        let name = self.post_name.clone().unwrap_or(PhpValue::Null);
        if "trash" != self.post_status && !is_empty(&name) {
            let add = api.filter(
                "add_trashed_suffix_to_trashed_posts",
                PhpValue::Bool(true),
                || vec![hv(&name), id_arg(id)],
            )?;
            if !is_empty(&add) {
                api.add_trashed_suffix_to_trashed_posts(&name, id)?;
            }
        }
        if "trash" == self.post_status
            && "trash" != self.previous_status
            && "new" != self.previous_status
        {
            self.post_name = Some(api.add_trashed_suffix_to_post(PhpValue::Int(int(id)))?);
        }
        let name = to_php_string(&self.post_name.clone().unwrap_or(PhpValue::Null));
        self.post_name = Some(api.unique_post_slug(
            &name,
            id,
            &self.post_status,
            &self.post_type,
            self.post_parent,
        )?);
        Ok(())
    }

    /// Step 6 (post.php 5162-5220): `$data`, emoji encoded for a `utf8` column, the `wp_insert_post_data` (or
    /// `wp_insert_attachment_data`) filter, then `wp_unslash()`.
    fn data(&mut self) -> Result<(), PostError> {
        let api = self.api;
        let parent = match self.post_parent {
            PostParent::Int(n) => PhpValue::Int(n),
            PostParent::Null => PhpValue::Null,
        };
        let mime = or(&self.postarr, "post_mime_type", s(""));
        let values = [
            self.post_author.clone(),
            s(&self.post_date),
            s(&self.post_date_gmt),
            self.post_content.clone(),
            self.post_content_filtered.clone(),
            self.post_title.clone(),
            self.post_excerpt.clone(),
            s(&self.post_status),
            s(&self.post_type),
            self.comment_status.clone(),
            self.ping_status.clone(),
            self.post_password.clone(),
            self.post_name.clone().unwrap_or(PhpValue::Null),
            self.to_ping.clone(),
            self.pinged.clone(),
            s(&self.post_modified),
            s(&self.post_modified_gmt),
            parent,
            PhpValue::Int(self.menu_order),
            mime,
            self.guid.clone(),
        ];
        let mut data: Items = DATA_KEYS.iter().map(|k| key(k)).zip(values).collect();
        for field in ["post_title", "post_content", "post_excerpt"] {
            if isset(&data, field).is_some() {
                let charset = api.d.charset.col_charset("posts", field)?;
                if matches!(charset.as_deref(), Some("utf8" | "utf8mb3")) {
                    let encoded = api
                        .d
                        .formatting
                        .encode_emoji(at(&data, field).expect("isset"))?;
                    set(&mut data, field, encoded);
                }
            }
        }
        let hook = if "attachment" == self.post_type {
            "wp_insert_attachment_data"
        } else {
            "wp_insert_post_data"
        };
        let postarr = PhpValue::Array(self.postarr.clone());
        let unsanitized = self.unsanitized.clone();
        let update = self.update;
        let filtered = api.filter(hook, PhpValue::Array(data), || {
            vec![hv(&postarr), hv(&unsanitized), HookValue::Bool(update)]
        })?;
        self.data = to_array(wp_unslash(filtered));
        Ok(())
    }

    /// Step 7 (post.php 5222-5281): the update, or the insert with its `import_id`. A failed write is WordPress's
    /// `db_update_error` or `db_insert_error`.
    fn write(&mut self) -> Flow<()> {
        let api = self.api;
        let repo = api.repository();
        let attachment = "attachment" == self.post_type;
        if self.update {
            let id = self.post_id;
            let data = PhpValue::Array(self.data.clone());
            api.action("pre_post_update", || vec![id_arg(id), hv(&data)])?;
            let rows = checked_rows(repo.update(&post_from_data(&self.data, id))?)?;
            if rows.is_none() {
                return self.refuse(PostWriteError::DbUpdate {
                    attachment,
                    last_error: String::new(),
                });
            }
        } else {
            let mut import = None;
            if !is_empty(&self.import_id) {
                let import_id = to_php_int(&self.import_id);
                if !repo.import_id_taken(import_id)? {
                    self.data.push((key("ID"), PhpValue::Int(import_id)));
                    import = Some(import_id);
                }
            }
            let data = PhpValue::Array(self.data.clone());
            api.action("pre_post_insert", || vec![hv(&data)])?;
            let Some(id) = repo.insert(&post_from_data(&self.data, PostId(0)), import)? else {
                hidden_failure()?;
                return self.refuse(PostWriteError::DbInsert {
                    attachment,
                    last_error: String::new(),
                });
            };
            self.post_id = id;
        }
        Ok(ControlFlow::Continue(()))
    }

    /// Step 8 (post.php 5283-5390): the slug of a published post written without one, the categories, tags,
    /// default terms and `tax_input`, `meta_input`, the GUID of a new post, an attachment's file and context, the
    /// featured image, then `clean_post_cache()`.
    fn after_write(&mut self) -> Result<(), PostError> {
        let api = self.api;
        let id = self.post_id;
        let status = text(at(&self.data, "post_status").unwrap_or(&PhpValue::Null));
        if empty_at(&self.data, "post_name") && !DRAFTS.contains(&status.as_str()) {
            let title = at(&self.data, "post_title")
                .cloned()
                .unwrap_or(PhpValue::Null);
            let sanitized =
                api.d
                    .formatting
                    .sanitize_title(&title, &PhpValue::Int(int(id)), "save")?;
            let name = api.unique_post_slug(
                &to_php_string(&sanitized),
                id,
                &status,
                &self.post_type,
                self.post_parent,
            )?;
            set(&mut self.data, "post_name", name.clone());
            checked_rows(
                api.repository()
                    .update_name(id, &Slug::new(to_php_string(&name)))?,
            )?;
            api.clean_post_cache(id)?;
        }

        self.terms()?;

        if let Some(PhpValue::Array(meta)) =
            at(&self.postarr, "meta_input").filter(|v| !is_empty(v))
        {
            for (field, value) in meta.clone() {
                api.update_meta(id, key_to_string(&field).as_bytes(), value)?;
            }
        }

        let current_guid = api.post_field("guid", id)?;
        if !self.update && is_str(&current_guid, "") {
            let guid = api.d.rewrite.permalink(id)?.unwrap_or_default();
            checked_rows(api.repository().update_guid(id, &guid)?)?;
        }

        if at(&self.postarr, "post_type").is_some_and(|t| is_str(t, "attachment")) {
            if !empty_at(&self.postarr, "file") {
                api.d
                    .uploads
                    .update_attached_file(id, at(&self.postarr, "file").expect("not empty"))?;
            }
            if !empty_at(&self.postarr, "context") {
                let context = at(&self.postarr, "context").cloned().expect("not empty");
                api.add_meta(id, b"_wp_attachment_context", context, true)?;
            }
        }

        self.thumbnail()?;
        api.clean_post_cache(id)
    }

    /// The categories, tags, default terms and `tax_input` (post.php 5296-5352).
    fn terms(&mut self) -> Result<(), PostError> {
        let api = self.api;
        let tax = &*api.d.taxonomy;
        let id = self.post_id;
        let post_type = PostTypeName::new(self.post_type.clone());
        if tax.is_object_in_taxonomy(&post_type, "category")? {
            api.set_post_categories(id, self.post_category.clone())?;
        }
        if let Some(tags) = isset(&self.postarr, "tags_input").cloned()
            && tax.is_object_in_taxonomy(&post_type, "post_tag")?
        {
            api.set_post_terms(id, tags, "post_tag")?;
        }

        let mut tax_input = at(&self.postarr, "tax_input").cloned();
        if "auto-draft" != self.post_status {
            for taxonomy in tax.default_terms(&post_type)? {
                let entry = |input: &Option<PhpValue>| -> Option<PhpValue> {
                    match input {
                        Some(PhpValue::Array(items)) => {
                            speedyspec_php_serialize::semantics::get(items, &taxonomy).cloned()
                        }
                        _ => None,
                    }
                };
                let put = |input: &mut Option<PhpValue>, value: PhpValue| {
                    let mut items = match input.take() {
                        Some(PhpValue::Array(items)) => items,
                        Some(other) => to_array(other),
                        None => Vec::new(),
                    };
                    set(&mut items, &taxonomy, value);
                    *input = Some(PhpValue::Array(items));
                };
                if let Some(value @ PhpValue::Array(_)) = entry(&tax_input) {
                    put(&mut tax_input, array_filter(value));
                }
                let args = PhpValue::Array(vec![(key("fields"), s("ids"))]);
                let terms = tax.get_object_terms(id, &taxonomy, &args)?;
                if !is_empty(&terms) && entry(&tax_input).is_none_or(|v| is_empty(&v)) {
                    put(&mut tax_input, terms);
                }
                if entry(&tax_input).is_none_or(|v| is_empty(&v)) {
                    let default = api.option(&format!("default_term_{taxonomy}"))?;
                    if !is_empty(&default) {
                        put(
                            &mut tax_input,
                            PhpValue::Array(vec![(
                                PhpKey::Int(0),
                                PhpValue::Int(to_php_int(&default)),
                            )]),
                        );
                    }
                }
            }
            if let Some(input) = &tax_input {
                set(&mut self.postarr, "tax_input", input.clone());
            }
        }

        if let Some(PhpValue::Array(input)) = tax_input.filter(|v| !is_empty(v)) {
            for (taxonomy, tags) in input {
                let taxonomy = key_to_string(&taxonomy);
                let Some(cap) = tax.assign_terms_cap(&taxonomy)? else {
                    let message = api
                        .d
                        .translator
                        .translate("Invalid taxonomy: %s.", None)
                        .replacen("%s", &taxonomy, 1);
                    api.d
                        .runtime
                        .doing_it_wrong("wp_insert_post", &message, "4.4.0")?;
                    continue;
                };
                let tags = match tags {
                    tags @ PhpValue::Array(_) => array_filter(tags),
                    other => other,
                };
                if api.d.users.current_user_can(&cap, &[])? {
                    tax.set_post_terms(id, &tags, &taxonomy, false)?;
                }
            }
        }
        Ok(())
    }

    /// The featured image (post.php 5370-5390): `_thumbnail_id` sets it, or `-1` removes it, where the type and the
    /// theme support thumbnails.
    fn thumbnail(&self) -> Result<(), PostError> {
        let Some(thumbnail) = isset(&self.postarr, "_thumbnail_id") else {
            return Ok(());
        };
        let api = self.api;
        let id = self.post_id;
        let supports = |post_type: &str| api.d.supports.supports(post_type, "thumbnail");
        let theme = |arg: &str| api.d.theme.current_theme_supports("post-thumbnails", arg);
        let mut support =
            (theme(&self.post_type)? && supports(&self.post_type)) || "revision" == self.post_type;
        let mime = or(&self.postarr, "post_mime_type", s(""));
        if !support && "attachment" == self.post_type && !is_empty(&mime) {
            if api.d.uploads.attachment_is("audio", id)? {
                support = supports("attachment:audio") || theme("attachment:audio")?;
            } else if api.d.uploads.attachment_is("video", id)? {
                support = supports("attachment:video") || theme("attachment:video")?;
            }
        }
        if support {
            let thumbnail = to_php_int(thumbnail);
            if -1 == thumbnail {
                api.d.uploads.delete_post_thumbnail(id)?;
            } else {
                api.d.uploads.set_post_thumbnail(id, thumbnail)?;
            }
        }
        Ok(())
    }

    /// `$post_before`'s object, built once: `post_updated` and `wp_after_insert_post` receive the same one.
    fn before_handle(&mut self) -> Result<HookValue, PostError> {
        if let Some(handle) = &self.post_before_handle {
            return Ok(handle.clone());
        }
        let handle = match &self.post_before {
            Some(before) => self
                .api
                .d
                .objects
                .handle(&PostObject::new(before.post.clone()))?,
            None => HookValue::Null,
        };
        self.post_before_handle = Some(handle.clone());
        Ok(handle)
    }

    /// Step 9 (post.php 5392-5543): `get_post()`, the page template, the transition (or the attachment actions),
    /// `edit_post`, `post_updated`, the `save_post` family and `wp_after_insert_post`. They share one object.
    fn finish(&mut self) -> Flow<WriteResult> {
        let api = self.api;
        let id = self.post_id;
        let Some(loaded) = api.d.posts.instance(int(id))? else {
            return Err(PostError::Db(format!(
                "post {} vanished after it was written",
                id.0
            )));
        };
        let mut object = PostObject::new(loaded.post.clone());

        if let Some(template) = at(&self.postarr, "page_template")
            .filter(|v| !is_empty(v))
            .cloned()
        {
            object = object.with_property("page_template", hv(&template));
            let templates = api.d.theme.page_templates(&object.post, &template)?;
            let listed = match (
                &templates,
                speedyspec_php_serialize::semantics::to_array_key(&template),
            ) {
                (PhpValue::Array(items), Some(k)) => items
                    .iter()
                    .any(|(ik, v)| *ik == k && !matches!(v, PhpValue::Null)),
                _ => false,
            };
            if !is_str(&template, "default") && !listed {
                if self.wp_error {
                    return Ok(ControlFlow::Break(Err(PostWriteError::InvalidPageTemplate)));
                }
                api.update_meta(id, b"_wp_page_template", s("default"))?;
            } else {
                api.update_meta(id, b"_wp_page_template", template)?;
            }
        }

        if !at(&self.postarr, "post_type").is_some_and(|t| is_str(t, "attachment")) {
            let status = text(at(&self.data, "post_status").unwrap_or(&PhpValue::Null));
            object = api.wp_transition_post_status(
                &PostStatusName::new(status),
                &PostStatusName::new(self.previous_status.clone()),
                &object,
            )?;
        } else {
            if self.update {
                api.action("edit_attachment", || vec![id_arg(id)])?;
                let after = api.object_of(id)?;
                let before = self.before_handle()?;
                api.action("attachment_updated", || vec![id_arg(id), after, before])?;
            } else {
                api.action("add_attachment", || vec![id_arg(id)])?;
            }
            return Ok(ControlFlow::Continue(Ok(id)));
        }

        let handle = api.d.objects.handle(&object)?;
        // `$post->post_type`, as the object holds it once the transition's callbacks have run.
        let post_type = match api.d.objects.read(&handle)? {
            Some(properties) => id_and_type(&properties).1,
            None => loaded.post.post_type.as_str().to_owned(),
        };
        let update = self.update;
        if update {
            api.action(&format!("edit_post_{post_type}"), || {
                vec![id_arg(id), handle.clone()]
            })?;
            api.action("edit_post", || vec![id_arg(id), handle.clone()])?;
            let after = api.object_of(id)?;
            let before = self.before_handle()?;
            api.action("post_updated", || vec![id_arg(id), after, before])?;
        }
        let saved = || vec![id_arg(id), handle.clone(), HookValue::Bool(update)];
        api.action(&format!("save_post_{post_type}"), saved)?;
        api.action("save_post", saved)?;
        api.action("wp_insert_post", saved)?;
        if self.fire_after_hooks {
            // wp_after_insert_post(): get_post() hands the raw object back as it is.
            let before = self.before_handle()?;
            api.action("wp_after_insert_post", || {
                vec![id_arg(id), handle.clone(), HookValue::Bool(update), before]
            })?;
        }
        Ok(ControlFlow::Continue(Ok(id)))
    }
}

/// `urlencode()`: letters, digits and `-_.` as they are, a space as `+`, every other byte `%XX` in upper case.
fn urlencode(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    for &b in bytes {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' => out.push(b),
            b' ' => out.push(b'+'),
            b => out.extend_from_slice(format!("%{b:02X}").as_bytes()),
        }
    }
    out
}

impl PostWritesApi {
    /// `wp_insert_post( $postarr, $wp_error, $fire_after_hooks )` (Template Method), its steps in WordPress's order:
    /// the defaults and `sanitize_post( 'db' )`; `wp_insert_post_empty_content`; status, categories, slug, dates,
    /// comment and ping status and the rest; `wp_insert_post_parent`, whose default callback breaks a loop through
    /// `wp_update_post()`; the trashed-slug suffixes and `wp_unique_post_slug()`; `$data` and `wp_insert_post_data`;
    /// the repository update or insert; the slug of a post written without one, terms, meta input, GUID, the
    /// attachment's file and the featured image, `clean_post_cache()`; then the page template, the transition,
    /// `edit_post`, `post_updated`, the `save_post` family and `wp_after_insert_post`, which share one object.
    ///
    /// The answer is the id, `0`, or the `WP_Error` WordPress returns (`Ok(Err(_))`); `Err` is a failure WordPress
    /// would not hide, after which nothing more is written.
    pub fn wp_insert_post(
        &self,
        postarr: PhpValue,
        wp_error: bool,
        fire_after_hooks: bool,
    ) -> Result<WriteResult, PostError> {
        Insert::new(self, postarr, wp_error, fire_after_hooks).run()
    }

    /// `get_post( $id )` as the object a hook receives: `null` when there is none.
    pub(crate) fn object_of(&self, id: PostId) -> Result<HookValue, PostError> {
        match self.d.posts.instance(int(id))? {
            Some(loaded) => self.d.objects.handle(&PostObject::new(loaded.post)),
            None => Ok(HookValue::Null),
        }
    }

    /// `wp_unique_post_slug( $slug, $post_id, $post_status, $post_type, $post_parent )`, its filters included, with
    /// the bad-slug filters through the Hook API and `get_post( 0 )` falling back to the global `$post`.
    pub(crate) fn unique_post_slug(
        &self,
        slug: &[u8],
        id: PostId,
        status: &str,
        post_type: &str,
        parent: PostParent,
    ) -> Result<PhpValue, PostError> {
        let deps = self.d.posts.deps();
        let bad = HookBadSlugs::new(self.hooks());
        let reader = SlugReader(&self.d.posts);
        let answer = unique_post_slug_filtered(
            self.hooks(),
            &SlugRequest {
                slug,
                post_id: id,
                status,
                post_type,
                parent,
            },
            &self.d.slugs.slug_context(),
            &SlugPorts {
                repository: &*deps.repository,
                posts: &reader,
                types: &*deps.types,
                truncation: &*self.d.truncation,
                bad: &bad,
            },
        );
        if let Some(e) = bad.take_error() {
            return Err(e);
        }
        answer
    }

    /// `wp_add_trashed_suffix_to_post_name_for_trashed_posts( $post_name, $post_id )` (post.php 8821-8838): every
    /// trashed post `get_posts()` finds with the slug takes the `__trashed` suffix.
    pub(crate) fn add_trashed_suffix_to_trashed_posts(
        &self,
        name: &PhpValue,
        id: PostId,
    ) -> Result<(), PostError> {
        let args = PhpValue::Array(vec![
            (key("name"), name.clone()),
            (key("post_status"), s("trash")),
            (key("post_type"), s("any")),
            (key("nopaging"), PhpValue::Bool(true)),
            (
                key("post__not_in"),
                PhpValue::Array(vec![(PhpKey::Int(0), PhpValue::Int(int(id)))]),
            ),
        ]);
        let found = self.d.query.get_posts(args)?;
        if let PhpValue::Array(posts) = found {
            for (_, post) in posts {
                self.add_trashed_suffix_to_post(post)?;
            }
        }
        Ok(())
    }

    /// `wp_add_trashed_suffix_to_post_name_for_post( $post )` (post.php 8855-8868): the slug as it is when it already
    /// ends in `__trashed`; otherwise `_wp_desired_post_slug` keeps it, and the post takes it cut to 191 bytes plus
    /// `__trashed`. Answers the slug.
    pub(crate) fn add_trashed_suffix_to_post(&self, post: PhpValue) -> Result<PhpValue, PostError> {
        let loaded = match &post {
            PhpValue::Int(id) => self.d.posts.instance(*id)?,
            _ => self.d.posts.resolve_value(post)?,
        };
        // WordPress reads `$post->post_name` of null there, a warning, and goes on with an empty name.
        let (id, name) = match &loaded {
            Some(l) => (l.post.id, l.post.name.as_bytes().to_vec()),
            None => (PostId(0), Vec::new()),
        };
        if name.ends_with(b"__trashed") {
            return Ok(s(&name));
        }
        self.add_meta(id, b"_wp_desired_post_slug", s(&name), false)?;
        let mut trashed = self.d.truncation.truncate(&name, 191);
        trashed.extend_from_slice(b"__trashed");
        checked_rows(
            self.repository()
                .update_name(id, &Slug::new(trashed.clone()))?,
        )?;
        self.clean_post_cache(id)?;
        Ok(s(&trashed))
    }

    /// `wp_set_post_categories( $post_id, $post_categories )` (post.php 6070-6118).
    pub(crate) fn set_post_categories(
        &self,
        id: PostId,
        categories: PhpValue,
    ) -> Result<(), PostError> {
        let posts = &self.d.posts;
        let post_type = posts.get_post_type(PostRef::Id(id))?;
        let status = posts.get_post_status(PostRef::Id(id))?;
        let categories = to_array(categories);
        let categories = if categories.is_empty() {
            let types = self.filter(
                "default_category_post_types",
                PhpValue::Array(Vec::new()),
                Vec::new,
            )?;
            let mut types = to_array(types);
            types = speedyspec_php_serialize::semantics::array_merge(
                types,
                vec![(PhpKey::Int(0), s("post"))],
            );
            let type_value = post_type
                .as_ref()
                .map_or(PhpValue::Bool(false), |t| s(t.as_str()));
            let listed = types
                .iter()
                .any(|(_, t)| crate::write::php::identical(t, &type_value));
            let in_category = match &post_type {
                Some(t) => self.d.taxonomy.is_object_in_taxonomy(t, "category")?,
                None => false,
            };
            if listed && in_category && !is_str(&status, "auto-draft") {
                vec![(PhpKey::Int(0), self.option("default_category")?)]
            } else {
                Vec::new()
            }
        } else if categories.len() == 1 && is_str(&categories[0].1, "") {
            return Ok(());
        } else {
            categories
        };
        self.d
            .taxonomy
            .set_post_terms(id, &PhpValue::Array(categories), "category", false)?;
        Ok(())
    }

    /// `wp_set_post_tags()` and the other `wp_set_post_terms( $post_id, $terms, $taxonomy )` callers.
    pub(crate) fn set_post_terms(
        &self,
        id: PostId,
        terms: PhpValue,
        taxonomy: &str,
    ) -> Result<(), PostError> {
        self.d
            .taxonomy
            .set_post_terms(id, &terms, taxonomy, false)?;
        Ok(())
    }
}

/// `$post->ID` and `$post->post_type` of an object a hook received, as PHP holds them now.
pub(crate) fn id_and_type(properties: &[(PhpKey, PhpValue)]) -> (PostId, String) {
    let id = at(properties, "ID").map_or(0, to_php_int);
    let post_type = at(properties, "post_type").map(text).unwrap_or_default();
    (post_id(id), post_type)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urlencode_follows_php() {
        assert_eq!(urlencode(b"a b-c_d.e~"), b"a+b-c_d.e%7E".to_vec());
        assert_eq!(urlencode("é".as_bytes()), b"%C3%A9".to_vec());
    }

    #[test]
    fn data_maps_to_the_entity() {
        let data: Items = vec![
            (key("post_author"), s("3")),
            (key("post_parent"), PhpValue::Null),
            (key("menu_order"), s("7")),
            (key("post_title"), s("T")),
        ];
        let post = post_from_data(&data, PostId(5));
        assert_eq!(post.id, PostId(5));
        assert_eq!(post.author, 3);
        assert_eq!(post.parent, PostParent::Null);
        assert_eq!(post.menu_order, 7);
        assert_eq!(post.title, b"T".to_vec());
    }
}
