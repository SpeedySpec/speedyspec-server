//! The post write use cases (`PostWritesApi`) over `MemoryPostRepository`, `WpHookApi` over the memory store, the
//! Rust object cache, phase 3's `MetadataApi` over memory meta, the registries `create_initial_post_types()` fills,
//! and port fakes that answer as WordPress 7.2-RC2 does on the recorded site (compat/posts-write/setup.php).
//!
//! The replay runs the recorded `insert-*`, `update-*`, `transition-*` and `set-type` steps that are pure in Rust
//! (no kses, no terms read back, no attachment files) and compares what each returned, every fixture callback's run
//! with its arguments, `wp_posts` and `wp_postmeta` whole, and the cron events. What WordPress's own revision code
//! and `post_updated` callbacks wrote is left out (they stay PHP until their phases). `transition-guid-reset` goes through `wp_publish_post()`, which phase 5b ports.

mod support;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use speedyspec_php_serialize::semantics::{key_to_string, to_php_int, to_php_string};
use speedyspec_php_serialize::{PhpKey, PhpValue, from_hook_value, serialize};
use speedyspec_wp_cron_contracts::{CronError, CronResult};
use speedyspec_wp_hooks::WpHookApi;
use speedyspec_wp_hooks_contracts::{Callback, HookError, HookName, HookValue};
use speedyspec_wp_meta::{MetaDeps, MetaKeyRegistry, MetadataApi, UnfilteredProtection};
use speedyspec_wp_meta_contracts::Metadata;
use speedyspec_wp_meta_contracts::{
    Installing, MetaKey, MetaRepositories, MetaType, ObjectId, ObjectSubtypes, SerializedValue,
};
use speedyspec_wp_meta_memory::MemoryMetaRepositories;
use speedyspec_wp_object_cache::{ObjectCachePorts, WpObjectCache};
use speedyspec_wp_options_contracts::{Options, OptionsError};
use speedyspec_wp_posts::{
    CacheInvalidation, InitialOptions, Microtime, PlainPostObjects, PostEntry, PostStatusRegistry,
    PostTypePorts, PostTypeRegistry, PostWritesApi, PostsApi, PostsDeps, Runtime, Scheduling,
    SlugContext, SlugSettings, Untranslated, WriteDeps, WriteFormatting, create_initial_post_types,
    register_future_post_hook, register_post_write_callbacks,
};
use speedyspec_wp_posts_contracts::{
    Charset, Clock, Comments, CurrentPost, LastPostField, MimeFilter, MimeType, PageRow, Post,
    PostArg, PostDateTime, PostError, PostId, PostObject, PostParent, PostQuery, PostRef,
    PostRepository, PostReturn, PostRow, PostStatusName, PostType, PostTypeName, PostTypeSupports,
    PostTypes, PostWriteError, Revisions, Rewrite, RewriteContext, RowValue, Slug, SlugTruncation,
    Taxonomy, Theme, Timezone, TitleSanitizer, Translator, Uploads, Users, WriteOutcome,
    WriteResult,
};
use speedyspec_wp_posts_memory::MemoryPostRepository;

// ---- the clock: 2026-11-27 12:00:00 GMT, `gmt_offset` 2 ----

const NOW: i64 = 1_795_780_800;
const OFFSET: i64 = 2 * 3600;

thread_local! {
    /// The frozen `time()`: [`NOW`], or a step's own `clock`.
    static CLOCK: Cell<i64> = const { Cell::new(NOW) };
}

fn now() -> i64 {
    CLOCK.with(Cell::get)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `Y-m-d H:i:s` (optionally ` GMT`) as a UTC timestamp, `strtotime()` normalising an out-of-range day.
fn parse(text: &[u8]) -> Option<i64> {
    let text = std::str::from_utf8(text).ok()?;
    let text = text.strip_suffix(" GMT").unwrap_or(text);
    let (date, time) = text.split_once(' ')?;
    let d: Vec<i64> = date
        .split('-')
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    let t: Vec<i64> = time
        .split(':')
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    if d.len() != 3 || t.len() != 3 {
        return None;
    }
    Some(
        days_from_civil(d[0], d[1], 1) * 86_400
            + (d[2] - 1) * 86_400
            + t[0] * 3600
            + t[1] * 60
            + t[2],
    )
}

fn format(ts: i64) -> String {
    let (y, m, d) = civil_from_days(ts.div_euclid(86_400));
    let s = ts.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}",
        s / 3600,
        s % 3600 / 60,
        s % 60
    )
}

thread_local! {
    /// When set, the local-time methods fail as a throwing `gmt_offset` option filter would.
    static CLOCK_THROWS: Cell<bool> = const { Cell::new(false) };
}

fn local_time() -> Result<(), PostError> {
    if CLOCK_THROWS.with(Cell::get) {
        return Err(PostError::Hook(HookError::message(
            "option_gmt_offset threw",
        )));
    }
    Ok(())
}

struct FixedClock;
impl Clock for FixedClock {
    fn now_local(&self) -> Result<String, PostError> {
        local_time()?;
        Ok(format(now() + OFFSET))
    }
    fn now_gmt(&self) -> String {
        format(now())
    }
    fn date_from_gmt(&self, gmt: &[u8]) -> Result<Vec<u8>, PostError> {
        local_time()?;
        Ok(parse(gmt).map_or(gmt.to_vec(), |t| format(t + OFFSET).into_bytes()))
    }
    fn gmt_from_date(&self, local: &[u8]) -> Result<Vec<u8>, PostError> {
        local_time()?;
        Ok(parse(local).map_or(local.to_vec(), |t| format(t - OFFSET).into_bytes()))
    }
    fn timestamp(&self, datetime: &[u8]) -> Option<i64> {
        parse(datetime)
    }
}

impl Microtime for FixedClock {
    fn microtime(&self) -> String {
        format!("0.00000000 {}", now())
    }
}

// ---- the registries, shared so a step can register more ----

#[derive(Default)]
struct Types(RefCell<PostTypeRegistry>);
impl PostTypes for Types {
    fn post_type(&self, name: &str) -> Option<PostType> {
        self.0.borrow().post_type(name)
    }
    fn post_types(&self) -> Vec<PostType> {
        self.0.borrow().post_types()
    }
}
impl PostTypeSupports for Types {
    fn supports(&self, post_type: &str, feature: &str) -> bool {
        self.0.borrow().supports(post_type, feature)
    }
    fn features(&self) -> Vec<(String, Vec<(String, PhpValue)>)> {
        self.0.borrow().features()
    }
}

struct NoRewrite;
impl RewriteContext for NoRewrite {
    fn rewrite_applies(&self) -> Result<bool, PostError> {
        Ok(false)
    }
}
struct Same;
impl TitleSanitizer for Same {
    fn sanitize_title_with_dashes(&self, title: &[u8]) -> Vec<u8> {
        title.to_vec()
    }
}

fn ports() -> PostTypePorts<'static> {
    PostTypePorts {
        translator: &Untranslated,
        rewrite: &NoRewrite,
        titles: &Same,
    }
}

// ---- port fakes ----

#[derive(Default)]
struct Global(RefCell<Option<PhpValue>>);
impl CurrentPost for Global {
    fn current(&self) -> Option<PhpValue> {
        self.0.borrow().clone()
    }
}

/// User 1, an administrator: every capability.
struct Admin;
impl Users for Admin {
    fn is_user_logged_in(&self) -> Result<bool, PostError> {
        Ok(true)
    }
    fn get_current_user_id(&self) -> Result<u64, PostError> {
        Ok(1)
    }
    fn current_user_can(&self, _: &PhpValue, _: &[PhpValue]) -> Result<bool, PostError> {
        Ok(true)
    }
}
impl speedyspec_wp_posts::Users for Admin {
    fn is_user_logged_in(&self) -> Result<bool, PostError> {
        Ok(true)
    }
    fn get_current_user_id(&self) -> Result<u64, PostError> {
        Ok(1)
    }
    fn current_user_can(&self, _: &PhpValue) -> Result<bool, PostError> {
        Ok(true)
    }
}

/// The seeded terms: post 1 in category 1 and tag 2 (`Tagged`); `category` and `post_tag` on posts, `genre` once
/// registered. Writes are recorded, not applied.
#[derive(Default)]
struct Terms {
    genre: Cell<bool>,
    set: RefCell<Vec<String>>,
}
impl Terms {
    fn on_post(&self, taxonomy: &str) -> bool {
        matches!(taxonomy, "category" | "post_tag") || (taxonomy == "genre" && self.genre.get())
    }
}
impl Taxonomy for Terms {
    fn update_object_term_cache(&self, _: &[PostId], _: &[PhpValue]) -> Result<(), PostError> {
        Ok(())
    }
    fn clean_object_term_cache(&self, _: &[PostId], _: &str) -> Result<(), PostError> {
        Ok(())
    }
    fn post_term_fields(
        &self,
        post: &PhpValue,
        taxonomy: &str,
        field: &str,
    ) -> Result<PhpValue, PostError> {
        let PhpValue::Object { properties, .. } = post else {
            return Ok(PhpValue::Array(Vec::new()));
        };
        let id = speedyspec_php_serialize::semantics::get(properties, "ID").map_or(0, to_php_int);
        let post_type = speedyspec_php_serialize::semantics::get(properties, "post_type")
            .map(to_php_string)
            .unwrap_or_default();
        if id != 1 || post_type != b"post" {
            return Ok(PhpValue::Array(Vec::new()));
        }
        Ok(PhpValue::Array(match (taxonomy, field) {
            ("category", "term_id") => vec![(PhpKey::Int(0), PhpValue::Int(1))],
            ("post_tag", "name") => vec![(PhpKey::Int(0), s("Tagged"))],
            _ => Vec::new(),
        }))
    }
    fn set_post_terms(
        &self,
        post: PostId,
        terms: &PhpValue,
        taxonomy: &str,
        append: bool,
    ) -> Result<PhpValue, PostError> {
        self.set.borrow_mut().push(format!(
            "{}:{taxonomy}:{}:{append}",
            post.0,
            String::from_utf8_lossy(&serialize(terms))
        ));
        Ok(PhpValue::Array(Vec::new()))
    }
    fn object_taxonomies(&self, post_type: &PostTypeName) -> Result<Vec<String>, PostError> {
        Ok(if post_type.as_str() == "post" {
            let mut names = vec!["category".to_owned(), "post_tag".to_owned()];
            if self.genre.get() {
                names.push("genre".to_owned());
            }
            names
        } else {
            Vec::new()
        })
    }
    fn default_terms(&self, _: &PostTypeName) -> Result<Vec<String>, PostError> {
        Ok(Vec::new())
    }
    fn assign_terms_cap(&self, taxonomy: &str) -> Result<Option<PhpValue>, PostError> {
        Ok(self.on_post(taxonomy).then(|| s("assign_terms")))
    }
    fn get_object_terms(&self, _: PostId, _: &str, _: &PhpValue) -> Result<PhpValue, PostError> {
        Ok(PhpValue::Array(Vec::new()))
    }
    fn get_the_terms(&self, post: PostId, taxonomy: &str) -> Result<PhpValue, PostError> {
        Ok(if post.0 == 1 && taxonomy == "post_tag" {
            PhpValue::Array(vec![(
                PhpKey::Int(0),
                PhpValue::Object {
                    class: "WP_Term".into(),
                    properties: vec![(k("name"), s("Tagged"))],
                },
            )])
        } else {
            PhpValue::Bool(false)
        })
    }
    fn is_object_in_taxonomy(
        &self,
        post_type: &PostTypeName,
        taxonomy: &str,
    ) -> Result<bool, PostError> {
        Ok(post_type.as_str() == "post" && self.on_post(taxonomy))
    }
    fn delete_object_term_relationships(&self, _: PostId, _: &[String]) -> Result<(), PostError> {
        Ok(())
    }
}
impl speedyspec_wp_posts::Taxonomy for Terms {
    fn update_object_term_cache(
        &self,
        ids: &[PostId],
        types: &[PhpValue],
    ) -> Result<(), PostError> {
        Taxonomy::update_object_term_cache(self, ids, types)
    }
    fn clean_object_term_cache(&self, ids: &[PostId], object_type: &str) -> Result<(), PostError> {
        Taxonomy::clean_object_term_cache(self, ids, object_type)
    }
    fn post_term_fields(
        &self,
        post: &PhpValue,
        taxonomy: &str,
        field: &str,
    ) -> Result<PhpValue, PostError> {
        Taxonomy::post_term_fields(self, post, taxonomy, field)
    }
}

/// `get_default_comment_status()` as WordPress answers it on the site: `closed` for pages and for types without the
/// support, else the `default_*_status` option (`open`).
struct DefaultComments {
    types: Rc<Types>,
    calls: RefCell<Vec<String>>,
    /// The seeded comments' statuses by post (setup.php): post 1 has comments 1 (approved) and 2 (not), post 7
    /// comment 3 (`post-trashed`).
    statuses: RefCell<HashMap<u64, Vec<(i64, &'static str)>>>,
    metadata: Rc<MetadataApi>,
}

impl DefaultComments {
    fn new(types: Rc<Types>, metadata: Rc<MetadataApi>) -> Self {
        DefaultComments {
            types,
            calls: RefCell::default(),
            statuses: RefCell::new(HashMap::from([
                (1, vec![(1, "1"), (2, "0")]),
                (7, vec![(3, "post-trashed")]),
            ])),
            metadata,
        }
    }
}
impl Comments for DefaultComments {
    fn default_comment_status(
        &self,
        post_type: &PostTypeName,
        comment_type: &str,
    ) -> Result<PhpValue, PostError> {
        let supports = if matches!(comment_type, "pingback" | "trackback") {
            "trackbacks"
        } else {
            "comments"
        };
        Ok(s(if post_type.as_str() == "page" {
            "closed"
        } else if self.types.supports(post_type.as_str(), supports) {
            "open"
        } else {
            "closed"
        }))
    }
    /// `wp_trash_post_comments()`: the statuses kept in `_wp_trash_meta_comments_status`, then `post-trashed`.
    fn trash_post_comments(&self, post: PostId) -> Result<(), PostError> {
        self.calls.borrow_mut().push(format!("trash {}", post.0));
        let mut statuses = self.statuses.borrow_mut();
        let Some(comments) = statuses.get_mut(&post.0) else {
            return Ok(());
        };
        let kept = PhpValue::Array(
            comments
                .iter()
                .map(|(id, status)| (PhpKey::Int(*id), s(status)))
                .collect(),
        );
        self.metadata
            .add(
                &MetaType::new("post").unwrap(),
                ObjectId(post.0),
                &MetaKey::from("_wp_trash_meta_comments_status"),
                kept,
                false,
            )
            .unwrap();
        for comment in comments.iter_mut() {
            comment.1 = "post-trashed";
        }
        Ok(())
    }
    /// `wp_untrash_post_comments()`: the kept statuses dropped with their meta.
    fn untrash_post_comments(&self, post: PostId) -> Result<(), PostError> {
        self.calls.borrow_mut().push(format!("untrash {}", post.0));
        if self.statuses.borrow().contains_key(&post.0) {
            self.metadata
                .delete(
                    &MetaType::new("post").unwrap(),
                    ObjectId(post.0),
                    &MetaKey::from("_wp_trash_meta_comments_status"),
                    None,
                    false,
                )
                .unwrap();
        }
        Ok(())
    }
    fn delete_post_comments(&self, post: PostId) -> Result<(), PostError> {
        self.calls.borrow_mut().push(format!("delete {}", post.0));
        self.statuses.borrow_mut().remove(&post.0);
        Ok(())
    }
    fn defer_counting(&self, defer: bool) -> Result<(), PostError> {
        self.calls.borrow_mut().push(format!("defer {defer}"));
        Ok(())
    }
}

/// `get_permalink()` with plain permalinks on `http://example.org`: pages and attachments their own links, a
/// registered type that is not built in `get_post_permalink()`'s, anything else a post's.
struct PlainPermalinks(Rc<MemoryPostRepository>, Rc<Types>);
impl Rewrite for PlainPermalinks {
    fn permalink(&self, post: PostId) -> Result<Option<Vec<u8>>, PostError> {
        let Some(found) = self.0.find(post)? else {
            return Ok(None);
        };
        let id = post.0;
        Ok(Some(
            match found.post_type.as_str() {
                "post" => format!("http://example.org/?p={id}"),
                "page" => format!("http://example.org/?page_id={id}"),
                "attachment" => format!("http://example.org/?attachment_id={id}"),
                other if matches!(other, "book" | "memo") && self.1.post_type(other).is_some() => {
                    format!("http://example.org/?post_type={other}&p={id}")
                }
                _ => format!("http://example.org/?p={id}"),
            }
            .into_bytes(),
        ))
    }
}

struct NoUploads;
impl Uploads for NoUploads {
    fn insert_attachment(&self, _: PhpValue, _: bool) -> Result<PhpValue, PostError> {
        panic!("no attachment is updated here")
    }
    fn delete_attachment(&self, _: PostId, _: bool) -> Result<PhpValue, PostError> {
        panic!("no attachment is deleted here")
    }
    fn update_attached_file(&self, _: PostId, _: &PhpValue) -> Result<(), PostError> {
        Ok(())
    }
    fn attachment_is(&self, _: &str, _: PostId) -> Result<bool, PostError> {
        Ok(false)
    }
    fn set_post_thumbnail(&self, _: PostId, _: i64) -> Result<(), PostError> {
        Ok(())
    }
    fn delete_post_thumbnail(&self, _: PostId) -> Result<(), PostError> {
        Ok(())
    }
}

/// No theme installed; `theme_page_templates` offers `tpl-wide.php`.
struct Templates;
impl Theme for Templates {
    fn page_templates(&self, _: &Post, _: &PhpValue) -> Result<PhpValue, PostError> {
        Ok(PhpValue::Array(vec![(k("tpl-wide.php"), s("Wide"))]))
    }
    fn current_theme_supports(&self, _: &str, _: &str) -> Result<bool, PostError> {
        Ok(false)
    }
}

/// `utf8mb4` columns: no emoji is encoded.
struct Utf8mb4;
impl Charset for Utf8mb4 {
    fn col_charset(&self, _: &str, _: &str) -> Result<Option<String>, PostError> {
        Ok(Some("utf8mb4".into()))
    }
}

/// `get_posts()` for the trashed posts with a name, over the memory table.
struct TrashedByName(Rc<MemoryPostRepository>);
impl PostQuery for TrashedByName {
    fn query(&self, _: PhpValue) -> Result<PhpValue, PostError> {
        panic!("no WP_Query here")
    }
    fn get_posts(&self, args: PhpValue) -> Result<PhpValue, PostError> {
        let PhpValue::Array(args) = args else {
            panic!("args")
        };
        let get = |n: &str| speedyspec_php_serialize::semantics::get(&args, n).cloned();
        assert_eq!(get("post_status"), Some(s("trash")));
        let name = get("name").map(|v| to_php_string(&v)).unwrap();
        let mut found = Vec::new();
        for id in 1..1000 {
            if let Some(post) = self.0.find(PostId(id))?
                && post.status.as_str() == "trash"
                && post.name.as_bytes() == name.as_slice()
            {
                found.push((
                    PhpKey::Int(found.len() as i64),
                    PostEntry::wp_post(PostEntry::raw_properties(&post)),
                ));
            }
        }
        Ok(PhpValue::Array(found))
    }
}

struct Cut;
impl SlugTruncation for Cut {
    fn truncate(&self, slug: &[u8], length: usize) -> Vec<u8> {
        slug[..slug.len().min(length)].to_vec()
    }
}

struct English;
impl Translator for English {
    fn translate(&self, text: &str, _: Option<&str>) -> Result<String, PostError> {
        Ok(text.to_owned())
    }
}

/// `sanitize_title()` with `sanitize_title_with_dashes()` for the ASCII titles the scenarios use.
struct Titles;
impl WriteFormatting for Titles {
    fn sanitize_title(
        &self,
        title: &PhpValue,
        fallback: &PhpValue,
        context: &str,
    ) -> Result<PhpValue, PostError> {
        let raw = to_php_string(title).to_ascii_lowercase();
        let mut out = Vec::new();
        for b in raw {
            match b {
                // utf8_uri_encode(): each byte of a multibyte character as `%xx`.
                0x80.. => out.extend(format!("%{b:02x}").into_bytes()),
                b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b'%' => out.push(b),
                b'.' if context == "save" => out.push(b'-'),
                b' ' | b'\t' | b'\n' => out.push(b'-'),
                _ => {}
            }
        }
        let mut collapsed: Vec<u8> = Vec::new();
        for b in out {
            if !(b == b'-' && collapsed.last() == Some(&b'-')) {
                collapsed.push(b);
            }
        }
        let trimmed: Vec<u8> = {
            let start = collapsed
                .iter()
                .position(|&b| b != b'-')
                .unwrap_or(collapsed.len());
            let end = collapsed
                .iter()
                .rposition(|&b| b != b'-')
                .map_or(start, |e| e + 1);
            collapsed[start..end.max(start)].to_vec()
        };
        Ok(if trimmed.is_empty() {
            fallback.clone()
        } else {
            PhpValue::Str(trimmed)
        })
    }
    fn encode_emoji(&self, content: &PhpValue) -> Result<PhpValue, PostError> {
        Ok(content.clone())
    }
}

/// The cron array as `(timestamp, hook, args)` events.
#[derive(Default)]
struct Cron(RefCell<Vec<(i64, String, PhpValue)>>);
impl Scheduling for Cron {
    fn clear_scheduled_hook(&self, hook: &str, args: &HookValue) -> Result<CronResult, CronError> {
        let args = from_hook_value(args).unwrap();
        let mut events = self.0.borrow_mut();
        let before = events.len();
        events.retain(|(_, h, a)| !(h == hook && *a == args));
        Ok(CronResult::Value(HookValue::Int(
            (before - events.len()) as i64,
        )))
    }
    fn schedule_single_event(
        &self,
        timestamp: &HookValue,
        hook: &str,
        args: &HookValue,
    ) -> Result<CronResult, CronError> {
        let HookValue::Int(ts) = timestamp else {
            return Ok(CronResult::Value(HookValue::Bool(false)));
        };
        self.0
            .borrow_mut()
            .push((*ts, hook.to_owned(), from_hook_value(args).unwrap()));
        Ok(CronResult::Value(HookValue::Bool(true)))
    }
    fn next_scheduled(&self, hook: &str, args: &HookValue) -> Result<HookValue, CronError> {
        let args = from_hook_value(args).unwrap();
        Ok(self
            .0
            .borrow()
            .iter()
            .find(|(_, h, a)| h == hook && *a == args)
            .map_or(HookValue::Bool(false), |(t, _, _)| HookValue::Int(*t)))
    }
}

/// `$wp_rewrite` on the site: plain permalinks (or a structure a test sets).
#[derive(Default)]
struct Rewriting(RefCell<Vec<u8>>);
impl SlugSettings for Rewriting {
    fn slug_context(&self) -> SlugContext {
        SlugContext {
            feeds: ["feed", "rdf", "rss", "rss2", "atom"]
                .map(|f| f.as_bytes().to_vec())
                .to_vec(),
            pagination_base: b"page".to_vec(),
            permalink_structure: self.0.borrow().clone(),
        }
    }
}

#[derive(Default)]
struct Rt(RefCell<Vec<String>>);
impl Runtime for Rt {
    fn defined(&self, _: &str) -> bool {
        false
    }
    fn doing_it_wrong(
        &self,
        function: &str,
        message: &str,
        version: &str,
    ) -> Result<(), PostError> {
        self.0
            .borrow_mut()
            .push(format!("{function}: {message} ({version})"));
        Ok(())
    }
    fn constant(&self, name: &str) -> Result<Option<PhpValue>, PostError> {
        Ok(match name {
            "EMPTY_TRASH_DAYS" => Some(PhpValue::Int(TRASH_DAYS.with(Cell::get))),
            _ => None,
        })
    }
}

thread_local! {
    /// `EMPTY_TRASH_DAYS`: 30 on the recorded site; a test sets 0 to turn the trash off.
    static TRASH_DAYS: Cell<i64> = const { Cell::new(30) };
}

/// The site's options as setup.php seeds them (`show_on_front` `page`, `page_on_front` 3, `sticky_posts` holding post
/// 1), read and written in memory. `update_option()` writes and answers true only for a changed value.
struct SiteOptions(RefCell<HashMap<String, HookValue>>);
impl Default for SiteOptions {
    fn default() -> Self {
        let text = |v: &str| HookValue::Str(v.into());
        let mut options = HashMap::new();
        for (name, value) in [
            ("default_category", "1"),
            ("default_pingback_flag", "1"),
            ("show_on_front", "page"),
            ("page_on_front", "3"),
            ("page_for_posts", "0"),
            ("wp_page_for_privacy_policy", "0"),
        ] {
            options.insert(name.to_owned(), text(value));
        }
        options.insert(
            "sticky_posts".into(),
            HookValue::Array(vec![(
                speedyspec_wp_hooks_contracts::Key::Int(0),
                HookValue::Int(1),
            )]),
        );
        SiteOptions(RefCell::new(options))
    }
}
impl SiteOptions {
    fn get(&self, name: &str) -> PhpValue {
        from_hook_value(
            &self
                .0
                .borrow()
                .get(name)
                .cloned()
                .unwrap_or(HookValue::Bool(false)),
        )
        .unwrap()
    }
}
impl Options for SiteOptions {
    fn get_option(
        &self,
        name: &str,
        default: Option<HookValue>,
    ) -> Result<HookValue, OptionsError> {
        Ok(self
            .0
            .borrow()
            .get(name)
            .cloned()
            .unwrap_or_else(|| default.unwrap_or(HookValue::Bool(false))))
    }
    fn add_option(
        &self,
        _: &str,
        _: HookValue,
        _: HookValue,
        _: HookValue,
    ) -> Result<bool, OptionsError> {
        Ok(false)
    }
    fn update_option(
        &self,
        name: &str,
        value: HookValue,
        _: HookValue,
    ) -> Result<bool, OptionsError> {
        let mut options = self.0.borrow_mut();
        let old = options.get(name).map(|v| from_hook_value(v).unwrap());
        let new = from_hook_value(&value).unwrap();
        // update_option() compares the serialized values (a scalar as its string).
        let same = |a: &PhpValue, b: &PhpValue| match (a, b) {
            (PhpValue::Array(_), _) | (_, PhpValue::Array(_)) => serialize(a) == serialize(b),
            _ => to_php_string(a) == to_php_string(b),
        };
        if old.as_ref().is_some_and(|old| same(old, &new)) {
            return Ok(false);
        }
        options.insert(name.to_owned(), value);
        Ok(true)
    }
    fn delete_option(&self, _: &str) -> Result<bool, OptionsError> {
        Ok(false)
    }
}

/// `wp_delete_post_revision( $id )`: `wp_delete_post( $id )` through the API, as revision.php does (the revision
/// code stays WordPress's until phase 6; this stands in for it).
#[derive(Default)]
struct DeletingRevisions(RefCell<std::rc::Weak<PostWritesApi>>, RefCell<Vec<u64>>);
impl Revisions for DeletingRevisions {
    fn save_post_revision(&self, _: PostId) -> Result<(), PostError> {
        Ok(())
    }
    fn delete_post_revision(&self, revision: PostId) -> Result<(), PostError> {
        self.1.borrow_mut().push(revision.0);
        if let Some(api) = self.0.borrow().upgrade() {
            api.wp_delete_post(revision.0 as i64, false)?;
        }
        Ok(())
    }
}

struct Never;
impl CacheInvalidation for Never {
    fn suspended(&self) -> bool {
        false
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

/// A repository that hides a `query` callback's exception behind its `update_name` answer, as `$wpdb` does.
struct HidingRepository(Rc<MemoryPostRepository>);

macro_rules! delegate {
    ($($name:ident($($arg:ident: $ty:ty),*) -> $ret:ty;)*) => {
        $( fn $name(&self, $($arg: $ty),*) -> $ret { self.0.$name($($arg),*) } )*
    };
}

impl PostRepository for HidingRepository {
    delegate! {
        engine() -> String;
        find(id: PostId) -> Result<Option<Post>, PostError>;
        insert(row: &PostRow) -> Result<WriteOutcome<PostId>, PostError>;
        attachment_slug_owner(slug: &Slug, except: PostId) -> Result<Option<Slug>, PostError>;
        hierarchical_slug_owner(slug: &Slug, post_type: &PostTypeName, except: PostId, parent: PostParent) -> Result<Option<Slug>, PostError>;
        flat_slug_owner(slug: &Slug, post_type: &PostTypeName, except: PostId) -> Result<Option<Slug>, PostError>;
        find_many(ids: &[PostId]) -> Result<Vec<Post>, PostError>;
        parent_ids(ids: &[PostId]) -> Result<Vec<(PostId, PostId)>, PostError>;
        pages_by_names(names: &[Slug], post_types: &[PostTypeName]) -> Result<Vec<PageRow>, PostError>;
        all_page_ids() -> Result<Vec<PostId>, PostError>;
        count_by_status(post_type: &PostTypeName, reader: Option<u64>) -> Result<Vec<(PostStatusName, u64)>, PostError>;
        count_attachments(mime_type: Option<&MimeFilter>) -> Result<(Vec<(MimeType, u64)>, u64), PostError>;
        last_post_time(field: LastPostField, post_types: &[PostTypeName], timezone: Timezone) -> Result<Option<PostDateTime>, PostError>;
        available_mime_types(post_type: &PostTypeName) -> Result<Vec<MimeType>, PostError>;
        update(id: PostId, row: &PostRow) -> Result<WriteOutcome<u64>, PostError>;
        update_guid(id: PostId, guid: &[u8]) -> Result<Option<u64>, PostError>;
        update_status(id: PostId, status: &PostStatusName) -> Result<Option<u64>, PostError>;
        update_pinged(id: PostId, pinged: &[u8]) -> Result<Option<u64>, PostError>;
        update_post_type(id: i64, post_type: &PostTypeName) -> Result<Option<u64>, PostError>;
        import_id_taken(id: i64) -> Result<bool, PostError>;
        raw_row(id: PostId) -> Result<Option<Post>, PostError>;
        children_of_type(parent: PostId, post_type: &PostTypeName) -> Result<Vec<Post>, PostError>;
        reparent_children(from: PostId, to: PostParent, post_type: &PostTypeName) -> Result<Option<u64>, PostError>;
        revision_ids(parent: PostId) -> Result<Vec<PostId>, PostError>;
        reparent_attachments(from: PostId, to: PostParent) -> Result<Option<u64>, PostError>;
        remove(id: PostId) -> Result<Option<u64>, PostError>;
        auto_drafts_older_than_a_week() -> Result<Vec<PostId>, PostError>;
    }
    fn update_name(&self, _: PostId, _: &Slug) -> Result<Option<u64>, PostError> {
        let _: speedyspec_wp_db_contracts::WpDbError = HookError::message("query threw").into();
        Ok(Some(0))
    }
}

// ---- the fixture ----

fn s(text: &str) -> PhpValue {
    PhpValue::Str(text.as_bytes().to_vec())
}

fn k(name: &str) -> PhpKey {
    PhpKey::Str(name.as_bytes().to_vec())
}

struct Fixture {
    api: Rc<PostWritesApi>,
    posts: Rc<PostsApi>,
    hooks: Rc<WpHookApi>,
    repo: Rc<MemoryPostRepository>,
    meta: Rc<MemoryMetaRepositories>,
    types: Rc<Types>,
    terms: Rc<Terms>,
    cron: Rc<Cron>,
    global: Rc<Global>,
    rewriting: Rc<Rewriting>,
    runtime: Rc<Rt>,
    options: Rc<SiteOptions>,
    comments: Rc<DefaultComments>,
    revisions: Rc<DeletingRevisions>,
    log: support::Invocations,
}

fn compat(file: &str) -> serde_json::Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../compat/posts-write")
        .join(file);
    serde_json::from_str(&std::fs::read_to_string(&path).expect("the recording")).unwrap()
}

thread_local! {
    static RECORDED: (serde_json::Value, serde_json::Value) =
        (compat("scenarios.json"), compat("wordpress-7.2-RC2.json"));
}

/// A recorded snapshot's rows of `table`, each as `(column, value)` pairs; `None` when the step did not snapshot it.
fn snapshot_table(snapshot: &serde_json::Value, table: &str) -> Option<Vec<Vec<(String, String)>>> {
    snapshot["array"]
        .as_array()
        .unwrap()
        .iter()
        .find(|pair| pair[0] == table)
        .map(|pair| {
            pair[1]["array"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| {
                    row[1]["array"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|c| {
                            (
                                c[0].as_str().unwrap().to_owned(),
                                c[1].as_str().unwrap_or("").to_owned(),
                            )
                        })
                        .collect()
                })
                .collect()
        })
}

fn snapshot_rows(snapshot: &serde_json::Value, table: &str) -> Vec<Vec<(String, String)>> {
    snapshot_table(snapshot, table).unwrap_or_default()
}

/// Whether a recorded fixture run came from the revision code (`wp_save_post_revision()` on `post_updated` and
/// `wp_after_insert_post`, which writes the revision through `wp_insert_post()`): an argument names a `revision`.
/// Revisions stay WordPress's until phase 6, reached through a port no Rust callback calls here, so the replay
/// leaves those runs out.
fn from_revision(args: &PhpValue) -> bool {
    let PhpValue::Array(args) = args else {
        return false;
    };
    args.iter().any(|(_, arg)| {
        let fields = match arg {
            PhpValue::Array(items) => {
                match speedyspec_php_serialize::semantics::get(items, "props") {
                    Some(PhpValue::Array(props)) => props,
                    _ => items,
                }
            }
            _ => return false,
        };
        speedyspec_php_serialize::semantics::get(fields, "post_type") == Some(&s("revision"))
    })
}

fn col<'a>(row: &'a [(String, String)], name: &str) -> &'a str {
    &row.iter().find(|(c, _)| c == name).unwrap().1
}

/// The seed: the rows `insert-defaults` saw before its insert (ids 1 to 25) and their meta.
fn seed() -> (Vec<Post>, Vec<(u64, String, String)>) {
    RECORDED.with(|(_, results)| {
        let snapshot = &results["results"]["insert-defaults"][0]["snapshot"];
        let posts = snapshot_rows(snapshot, "wp_posts")
            .into_iter()
            .filter(|row| col(row, "ID").parse::<u64>().unwrap() < 26)
            .map(|row| {
                let date = |n: &str| PostDateTime::new(col(&row, n).to_owned());
                Post {
                    id: PostId(col(&row, "ID").parse().unwrap()),
                    author: col(&row, "post_author").parse().unwrap(),
                    date: speedyspec_wp_posts_contracts::PostDate {
                        local: date("post_date"),
                        gmt: date("post_date_gmt"),
                    },
                    content: col(&row, "post_content").into(),
                    title: col(&row, "post_title").into(),
                    excerpt: col(&row, "post_excerpt").into(),
                    status: PostStatusName::new(col(&row, "post_status")),
                    comment_status: speedyspec_wp_posts_contracts::CommentStatus::new(col(
                        &row,
                        "comment_status",
                    )),
                    ping_status: speedyspec_wp_posts_contracts::PingStatus::new(col(
                        &row,
                        "ping_status",
                    )),
                    password: col(&row, "post_password").into(),
                    name: Slug::from(col(&row, "post_name")),
                    to_ping: col(&row, "to_ping").into(),
                    pinged: col(&row, "pinged").into(),
                    modified: speedyspec_wp_posts_contracts::PostDate {
                        local: date("post_modified"),
                        gmt: date("post_modified_gmt"),
                    },
                    content_filtered: col(&row, "post_content_filtered").into(),
                    parent: PostParent::Int(col(&row, "post_parent").parse().unwrap()),
                    guid: col(&row, "guid").into(),
                    menu_order: col(&row, "menu_order").parse().unwrap(),
                    post_type: PostTypeName::new(col(&row, "post_type")),
                    mime_type: MimeType::new(col(&row, "post_mime_type")),
                    comment_count: col(&row, "comment_count").parse().unwrap(),
                }
            })
            .collect();
        let meta = snapshot_rows(snapshot, "wp_postmeta")
            .into_iter()
            .map(|row| {
                (
                    col(&row, "post_id").parse().unwrap(),
                    col(&row, "meta_key").to_owned(),
                    col(&row, "meta_value").to_owned(),
                )
            })
            .collect();
        (posts, meta)
    })
}

fn fixture() -> Fixture {
    fixture_with(|repo| repo)
}

fn fixture_with(wrap: impl FnOnce(Rc<MemoryPostRepository>) -> Rc<dyn PostRepository>) -> Fixture {
    let hooks = support::hooks();
    let cache = Rc::new(WpObjectCache::new(ObjectCachePorts::rust(), None));
    let repo = Rc::new(MemoryPostRepository::new());
    let (posts, meta_rows) = seed();
    for post in &posts {
        // The seeded row as stored: its `ID` and `comment_count` too, which a row may carry.
        let row = PostRow::from_post(post)
            .with("ID", RowValue::Int(post.id.0 as i64))
            .with("comment_count", RowValue::Int(post.comment_count));
        repo.insert(&row).unwrap();
    }
    let meta = Rc::new(MemoryMetaRepositories::new(&["post"]));
    let post_meta = meta.for_type(&MetaType::new("post").unwrap()).unwrap();
    for (id, key, value) in meta_rows {
        post_meta
            .add(
                ObjectId(id),
                &MetaKey::from(key.as_str()),
                Some(&SerializedValue::new(value.into_bytes())),
            )
            .unwrap();
    }
    let metadata = MetadataApi::new(MetaDeps {
        hooks: hooks.clone(),
        cache: cache.clone(),
        repositories: meta.clone(),
        keys: Rc::new(MetaKeyRegistry::new()),
        subtypes: Rc::new(PostSubtype),
        protection: Rc::new(UnfilteredProtection),
        installing: Rc::new(NotInstalling),
    });
    let types: Rc<Types> = Rc::default();
    let mut statuses = PostStatusRegistry::new();
    create_initial_post_types(
        &mut types.0.borrow_mut(),
        &mut statuses,
        &InitialOptions {
            attachment_pages_enabled: false,
        },
        &ports(),
    )
    .unwrap();
    let global: Rc<Global> = Rc::default();
    let terms: Rc<Terms> = Rc::default();
    let options: Rc<SiteOptions> = Rc::default();
    let comments = Rc::new(DefaultComments::new(types.clone(), metadata.clone()));
    let revisions: Rc<DeletingRevisions> = Rc::default();
    let posts = PostsApi::new(PostsDeps {
        hooks: hooks.clone(),
        cache,
        repository: wrap(repo.clone()),
        metadata,
        types: types.clone(),
        statuses: Rc::new(statuses),
        current: global.clone(),
        formatting: Rc::new(support::RustFormatting::new(hooks.clone())),
        users: Rc::new(Admin),
        terms: terms.clone(),
        clock: Rc::new(FixedClock),
        invalidation: Rc::new(Never),
        options: options.clone(),
    });
    let cron: Rc<Cron> = Rc::default();
    let rewriting: Rc<Rewriting> = Rc::default();
    let runtime: Rc<Rt> = Rc::default();
    let api = PostWritesApi::new(WriteDeps {
        posts: posts.clone(),
        supports: types.clone(),
        clock: Rc::new(FixedClock),
        users: Rc::new(Admin),
        taxonomy: terms.clone(),
        comments: comments.clone(),
        rewrite: Rc::new(PlainPermalinks(repo.clone(), types.clone())),
        uploads: Rc::new(NoUploads),
        theme: Rc::new(Templates),
        charset: Rc::new(Utf8mb4),
        query: Rc::new(TrashedByName(repo.clone())),
        truncation: Rc::new(Cut),
        translator: Rc::new(English),
        formatting: Rc::new(Titles),
        cron: cron.clone(),
        objects: Rc::new(PlainPostObjects),
        slugs: rewriting.clone(),
        runtime: runtime.clone(),
        revisions: revisions.clone(),
        meta: meta.clone(),
    });
    *revisions.0.borrow_mut() = Rc::downgrade(&api);
    // setup.php's default filters for the writers, in its order, as Rust callbacks.
    add_default_filters(&hooks, &posts);
    // default-filters.php's writers' callbacks, then each core type's `future_{type}` hook.
    register_post_write_callbacks(&*hooks, &api).unwrap();
    for post_type in types.post_types() {
        register_future_post_hook(&*hooks, &api, post_type.name()).unwrap();
    }
    Fixture {
        api,
        posts,
        hooks,
        repo,
        meta,
        types,
        terms,
        cron,
        global,
        rewriting,
        runtime,
        options,
        comments,
        revisions,
        log: Rc::default(),
    }
}

/// setup.php's default filters the writers rely on, as Rust callbacks: `sanitize_key` on the four `pre_post_*` name
/// fields, `sanitize_mime_type` on the mime type, `wp_cache_set_posts_last_changed` on the post meta actions, and
/// `trim` on `title_save_pre`. The `pre_post_guid` filters leave the recorded URLs as they are, so they are not added.
/// A filter callback's body.
type Run = Box<dyn Fn(&[HookValue]) -> Result<HookValue, HookError>>;

fn add_default_filters(hooks: &Rc<WpHookApi>, posts: &Rc<PostsApi>) {
    let add = |hook: &str, id: &str, run: Run| {
        hooks
            .add_filter(
                &HookName::from(hook),
                Callback::rust(id, move |args: &[HookValue]| run(args)),
                &HookValue::Int(10),
                1,
            )
            .unwrap();
    };
    let text = |args: &[HookValue]| to_php_string(&from_hook_value(&args[0]).unwrap());
    for hook in [
        "pre_post_type",
        "pre_post_status",
        "pre_post_comment_status",
        "pre_post_ping_status",
    ] {
        add(
            hook,
            "sanitize_key",
            Box::new(move |args| {
                Ok(HookValue::Str(speedyspec_wp_posts::sanitize_key(&text(
                    args,
                ))))
            }),
        );
    }
    for hook in ["pre_post_mime_type", "post_mime_type"] {
        add(
            hook,
            "sanitize_mime_type",
            Box::new(move |args| {
                let kept: Vec<u8> = text(args)
                    .into_iter()
                    .filter(|b| b.is_ascii_alphanumeric() || b"-+*./".contains(b))
                    .collect();
                Ok(HookValue::Str(String::from_utf8(kept).unwrap()))
            }),
        );
    }
    for hook in ["added_post_meta", "updated_post_meta", "deleted_post_meta"] {
        let posts = posts.clone();
        add(
            hook,
            "wp_cache_set_posts_last_changed",
            Box::new(move |_| {
                posts.wp_cache_set_posts_last_changed().unwrap();
                Ok(HookValue::Null)
            }),
        );
    }
    add(
        "title_save_pre",
        "trim",
        Box::new(move |args| {
            let t = text(args);
            let keep = |b: &u8| !b" \t\n\r\0\x0B".contains(b);
            let start = t.iter().position(keep).unwrap_or(t.len());
            let end = t.iter().rposition(keep).map_or(start, |e| e + 1);
            Ok(speedyspec_php_serialize::to_hook_value(&PhpValue::Str(
                t[start..end].to_vec(),
            )))
        }),
    );
}

impl Fixture {
    /// `speedyspec_compat_posts_write_register()`: the `genre` taxonomy, the hierarchical `book` type and the `memo`
    /// type, which is public but not publicly queryable.
    fn register_extra_types(&self) {
        self.terms.genre.set(true);
        let flag = |b: bool| PhpValue::Bool(b);
        for (name, args) in [
            (
                "book",
                vec![
                    (k("public"), flag(true)),
                    (k("hierarchical"), flag(true)),
                    (k("rewrite"), flag(false)),
                    (k("query_var"), flag(false)),
                ],
            ),
            (
                "memo",
                vec![
                    (k("public"), flag(true)),
                    (k("publicly_queryable"), flag(false)),
                    (k("rewrite"), flag(false)),
                    (k("query_var"), flag(false)),
                ],
            ),
        ] {
            let name = PostTypeName::new(name);
            self.types
                .0
                .borrow_mut()
                .register(name.clone(), args, &ports())
                .unwrap();
            register_future_post_hook(&*self.hooks, &self.api, &name).unwrap();
        }
    }

    fn insert(&self, postarr: PhpValue) -> WriteResult {
        self.api.wp_insert_post(postarr, true, true).unwrap()
    }

    fn row(&self, id: u64) -> Post {
        self.repo.find(PostId(id)).unwrap().unwrap()
    }

    fn meta_of(&self, id: u64, key: &str) -> Vec<String> {
        self.meta
            .for_type(&MetaType::new("post").unwrap())
            .unwrap()
            .for_objects(&[ObjectId(id)])
            .unwrap()
            .into_iter()
            .filter(|r| r.key.as_bytes() == key.as_bytes())
            .map(|r| String::from_utf8_lossy(r.value.unwrap().as_bytes()).into_owned())
            .collect()
    }
}

fn array(pairs: Vec<(&str, PhpValue)>) -> PhpValue {
    PhpValue::Array(pairs.into_iter().map(|(n, v)| (k(n), v)).collect())
}

// ---- the replay ----

/// A plain JSON value from scenarios.json as the driver gets it: the recorder re-serializes each scenario with
/// `serde_json`, so an object's keys arrive sorted, and PHP decodes it into an array in that order.
fn plain(value: &serde_json::Value, vars: &HashMap<String, Var>) -> Var {
    match value {
        serde_json::Value::Object(map) if map.len() == 1 && map.contains_key("var") => {
            vars[map["var"].as_str().unwrap()].clone()
        }
        other => Var::Value(plain_value_with(other, vars)),
    }
}

/// A plain JSON value, each nested `{"var": name}` replaced by the value kept as `name`.
fn plain_value_with(value: &serde_json::Value, vars: &HashMap<String, Var>) -> PhpValue {
    match value {
        serde_json::Value::Object(map) if map.len() == 1 && map.contains_key("var") => {
            plain(value, vars).value()
        }
        serde_json::Value::Array(items) => PhpValue::Array(
            items
                .iter()
                .enumerate()
                .map(|(i, v)| (PhpKey::Int(i as i64), plain_value_with(v, vars)))
                .collect(),
        ),
        serde_json::Value::Object(map) => PhpValue::Array(
            map.iter()
                .map(|(n, v)| (support::php_key(n), plain_value_with(v, vars)))
                .collect(),
        ),
        other => plain_scalar(other),
    }
}

fn plain_scalar(value: &serde_json::Value) -> PhpValue {
    match value {
        serde_json::Value::Null => PhpValue::Null,
        serde_json::Value::Bool(b) => PhpValue::Bool(*b),
        serde_json::Value::Number(n) => n
            .as_i64()
            .map_or_else(|| PhpValue::Float(n.as_f64().unwrap()), PhpValue::Int),
        serde_json::Value::String(t) => s(t),
        _ => unreachable!("arrays and objects are plain_value_with()'s"),
    }
}

#[derive(Clone)]
enum Var {
    Value(PhpValue),
    Post(Box<PostObject>),
}

impl Var {
    fn value(self) -> PhpValue {
        match self {
            Var::Value(v) => v,
            Var::Post(_) => panic!("a post where a value was expected"),
        }
    }
}

/// An object as the recorder reports it: `array( 'class' => …, 'props' => … )`, at any depth.
fn reported(value: PhpValue) -> PhpValue {
    match value {
        PhpValue::Object { class, properties } => array(vec![
            ("class", s(&class)),
            (
                "props",
                PhpValue::Array(
                    properties
                        .into_iter()
                        .map(|(k, v)| (k, reported(v)))
                        .collect(),
                ),
            ),
        ]),
        PhpValue::Array(items) => {
            PhpValue::Array(items.into_iter().map(|(k, v)| (k, reported(v))).collect())
        }
        other => other,
    }
}

/// Where two values first differ, as a path with both sides; `None` when they are equal.
fn first_difference(got: &PhpValue, want: &PhpValue, path: String) -> Option<String> {
    match (got, want) {
        (PhpValue::Array(a), PhpValue::Array(b)) => {
            for (i, (ka, va)) in a.iter().enumerate() {
                let here = format!("{path}[{}]", key_to_string(ka));
                match b.get(i) {
                    Some((kb, vb)) if kb == ka => {
                        if let Some(d) = first_difference(va, vb, here) {
                            return Some(d);
                        }
                    }
                    other => {
                        return Some(format!(
                            "{here}: got key {ka:?}, want {:?}",
                            other.map(|(k, _)| k)
                        ));
                    }
                }
            }
            (b.len() > a.len()).then(|| format!("{path}: missing {:?}", b[a.len()].0))
        }
        (a, b) if a == b => None,
        (a, b) => Some(format!("{path}: got {a:?}, want {b:?}")),
    }
}

/// `wp_posts` as the recorder snapshots it: every row, every column as a string.
fn table(f: &Fixture) -> Vec<Vec<(String, String)>> {
    let mut rows = Vec::new();
    for id in 1..1000 {
        let Some(post) = f.repo.find(PostId(id)).unwrap() else {
            continue;
        };
        let raw = PostEntry::raw_properties(&post);
        rows.push(
            raw.into_iter()
                .filter(|(k, _)| key_to_string(k) != "filter")
                .map(|(k, v)| {
                    (
                        key_to_string(&k),
                        String::from_utf8_lossy(&to_php_string(&v)).into_owned(),
                    )
                })
                .collect::<Vec<_>>(),
        );
    }
    rows
}

fn sorted(mut row: Vec<(String, String)>) -> Vec<(String, String)> {
    row.sort();
    row
}

fn meta_table(f: &Fixture) -> Vec<(String, String, String)> {
    let ids: Vec<ObjectId> = (1..1000).map(ObjectId).collect();
    f.meta
        .for_type(&MetaType::new("post").unwrap())
        .unwrap()
        .for_objects(&ids)
        .unwrap()
        .into_iter()
        .map(|r| {
            (
                r.object_id.0.to_string(),
                String::from_utf8_lossy(r.key.as_bytes()).into_owned(),
                r.value
                    .map(|v| String::from_utf8_lossy(v.as_bytes()).into_owned())
                    .unwrap_or_default(),
            )
        })
        .collect()
}

/// The recorded cron events, `(timestamp, hook, args)`, sorted.
fn recorded_cron(value: &serde_json::Value) -> Vec<(i64, String, PhpValue)> {
    let PhpValue::Array(events) = support::decode(value).unwrap() else {
        panic!("cron")
    };
    let mut out: Vec<_> = events
        .into_iter()
        .map(|(_, e)| {
            let PhpValue::Array(e) = e else {
                panic!("event")
            };
            let get = |n: &str| {
                speedyspec_php_serialize::semantics::get(&e, n)
                    .cloned()
                    .unwrap()
            };
            (
                to_php_int(&get("timestamp")),
                String::from_utf8_lossy(&to_php_string(&get("hook"))).into_owned(),
                get("args"),
            )
        })
        .collect();
    out.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    out
}

/// The recorded error code of a `WP_Error` kept as `name`.
fn recorded_error(state: &serde_json::Value, name: &str) -> String {
    let entry = state["array"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p[0] == format!("kept:{name}"))
        .unwrap();
    let object = support::decode_kept(&entry[1]);
    let PhpValue::Object { properties, .. } = object else {
        panic!()
    };
    let PhpValue::Array(errors) =
        speedyspec_php_serialize::semantics::get(&properties, "errors").unwrap()
    else {
        panic!()
    };
    key_to_string(&errors[0].0)
}

/// The fixture callbacks code that is not ported fires (the comment functions, `wp_delete_attachment()`): the
/// replay goes through their ports, which fire nothing.
const FROM_UNPORTED: [&str; 6] = [
    "rec:delete_comment",
    "rec:trash_post_comments",
    "rec:trashed_post_comments",
    "rec:untrash_post_comments",
    "rec:untrashed_post_comments",
    "rec:delete_attachment",
];

/// A value a step kept with `as`, as the recorder reported it (`None` when it did not change in that step).
fn kept_value(state: &serde_json::Value, name: &str) -> Option<PhpValue> {
    state["array"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p[0] == format!("kept:{name}"))
        .map(|entry| support::decode_kept(&entry[1]))
}

/// Inserts each revision row (id 26 and up) of a recorded `wp_posts` snapshot that the memory table lacks.
fn copy_revisions(f: &Fixture, rows: &[Vec<(String, String)>]) {
    for row in rows {
        let id: u64 = col(row, "ID").parse().unwrap();
        if col(row, "post_type") != "revision"
            || id < 26
            || f.repo.find(PostId(id)).unwrap().is_some()
        {
            continue;
        }
        let mut post_row = PostRow::default();
        for (column, value) in row {
            post_row = post_row.with(
                column.as_str(),
                match column.as_str() {
                    "ID" | "post_author" | "post_parent" | "menu_order" | "comment_count" => {
                        RowValue::Int(value.parse().unwrap())
                    }
                    _ => RowValue::Str(value.as_bytes().to_vec()),
                },
            );
        }
        f.repo.insert(&post_row).unwrap();
    }
}

fn write_answer(result: WriteResult) -> Result<i64, &'static str> {
    match result {
        Ok(id) => Ok(id.0 as i64),
        Err(e) => Err(e.code().unwrap()),
    }
}

/// Replays one recorded scenario, step by step, comparing everything a step recorded.
fn replay(name: &str) {
    RECORDED.with(|(defined, results)| {
        let scenario = defined["scenarios"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == name)
            .unwrap_or_else(|| panic!("no scenario {name}"));
        let recorded = results["results"][name].as_array().unwrap();
        let f = fixture();
        let mut vars: HashMap<String, Var> = HashMap::new();
        for (n, (step, result)) in scenario["steps"]
            .as_array()
            .unwrap()
            .iter()
            .zip(recorded)
            .enumerate()
        {
            let at = format!("{name} step {n} ({})", step["call"].as_str().unwrap());
            let args = step["args"].as_array().unwrap();
            f.log.borrow_mut().clear();
            let arg = |i: usize| args.get(i).map(|a| plain(a, &vars));
            let flag = |i: usize, default: bool| {
                arg(i).map_or(default, |v| matches!(v.value(), PhpValue::Bool(true)))
            };
            CLOCK.with(|c| {
                c.set(
                    step["clock"]
                        .as_i64()
                        .or(scenario["clock"].as_i64())
                        .unwrap_or(NOW),
                )
            });
            let post_arg = |i: usize| match to_php_int(&arg(i).unwrap().value()) {
                0 => PostArg::Ref(PostRef::Current),
                id => PostArg::Ref(PostRef::Id(PostId(id as u64))),
            };
            let kept = |var: &str| kept_value(&result["state"], var);
            let returned: Option<PhpValue> = match step["call"].as_str().unwrap() {
                "add_action" | "add_filter" => {
                    let hook = args[0].as_str().unwrap();
                    let priority = args.get(2).and_then(|p| p.as_i64()).unwrap_or(10);
                    let accepted = args.get(3).and_then(|p| p.as_i64()).unwrap_or(1);
                    match &args[1] {
                        spec @ serde_json::Value::Object(_) => {
                            support::add_fixture(&f.hooks, &f.log, hook, spec, priority, accepted)
                        }
                        serde_json::Value::String(callback) => {
                            let run: fn(&[HookValue]) -> Result<HookValue, HookError> =
                                match callback.as_str() {
                                    "speedyspec_compat_posts_write_forced_slug" => |args| {
                                        let slug = from_hook_value(&args[1]).unwrap();
                                        let mut out = b"forced-".to_vec();
                                        out.extend(to_php_string(&slug));
                                        Ok(speedyspec_php_serialize::to_hook_value(&PhpValue::Str(
                                            out,
                                        )))
                                    },
                                    "speedyspec_compat_posts_write_always_empty" => {
                                        |_| Ok(HookValue::Bool(true))
                                    }
                                    // wp_untrash_post_set_previous_status( $new, $post_id, $previous ).
                                    "wp_untrash_post_set_previous_status" => {
                                        |args| Ok(args[2].clone())
                                    }
                                    other => panic!("{at}: callback {other} is not replayed"),
                                };
                            f.hooks
                                .add_filter(
                                    &HookName::from(hook),
                                    Callback::rust(callback, run),
                                    &HookValue::Int(priority),
                                    accepted,
                                )
                                .unwrap();
                        }
                        other => panic!("{at}: {other}"),
                    }
                    Some(PhpValue::Bool(true))
                }
                "speedyspec_compat_posts_write_register" => {
                    f.register_extra_types();
                    Some(PhpValue::Bool(true))
                }
                "wp_insert_post" | "wp_update_post" => {
                    let postarr = arg(0).unwrap().value();
                    let wp_error = flag(1, false);
                    let after = flag(2, true);
                    let outcome = if step["call"] == "wp_insert_post" {
                        f.api.wp_insert_post(postarr, wp_error, after)
                    } else {
                        f.api.wp_update_post(postarr, wp_error, after)
                    };
                    let answer = write_answer(outcome.unwrap_or_else(|e| panic!("{at}: {e}")));
                    match answer {
                        Ok(id) => {
                            if let Some(var) = step["as"].as_str() {
                                vars.insert(var.to_owned(), Var::Value(PhpValue::Int(id)));
                            }
                            Some(PhpValue::Int(id))
                        }
                        Err(code) => {
                            let var = step["as"].as_str().unwrap();
                            assert_eq!(
                                code,
                                recorded_error(&result["state"], var),
                                "{at}: the error"
                            );
                            None
                        }
                    }
                }
                "get_post" => {
                    let id = to_php_int(&arg(0).unwrap().value());
                    let post = f
                        .posts
                        .get_post(speedyspec_wp_posts_contracts::PostRef::Id(PostId(
                            id as u64,
                        )))
                        .unwrap()
                        .unwrap();
                    vars.insert(
                        step["as"].as_str().unwrap().to_owned(),
                        Var::Post(Box::new(PostObject::new(post))),
                    );
                    None
                }
                "get_post_type" => {
                    let id = to_php_int(&arg(0).unwrap().value());
                    let found = f
                        .posts
                        .get_post_type(speedyspec_wp_posts_contracts::PostRef::Id(PostId(
                            id as u64,
                        )))
                        .unwrap();
                    Some(found.map_or(PhpValue::Bool(false), |t| s(t.as_str())))
                }
                "wp_transition_post_status" => {
                    let new = PostStatusName::new(String::from_utf8_lossy(&to_php_string(
                        &arg(0).unwrap().value(),
                    )));
                    let old = PostStatusName::new(String::from_utf8_lossy(&to_php_string(
                        &arg(1).unwrap().value(),
                    )));
                    let Some(Var::Post(post)) = arg(2) else {
                        panic!("{at}: a post")
                    };
                    f.api.wp_transition_post_status(&new, &old, &post).unwrap();
                    Some(PhpValue::Null)
                }
                "set_post_type" => {
                    let id = to_php_int(&arg(0).unwrap().value());
                    let post_type = arg(1).map_or(s("post"), Var::value);
                    let rows = f.api.set_post_type(id, &post_type).unwrap();
                    Some(rows.map_or(PhpValue::Bool(false), |n| PhpValue::Int(n as i64)))
                }
                "wp_delete_post" | "wp_trash_post" | "wp_untrash_post" => {
                    let answer = match step["call"].as_str().unwrap() {
                        "wp_delete_post" => {
                            let id = to_php_int(&arg(0).unwrap().value());
                            f.api.wp_delete_post(id, flag(1, false))
                        }
                        "wp_trash_post" => f.api.wp_trash_post(post_arg(0)),
                        _ => f.api.wp_untrash_post(post_arg(0)),
                    }
                    .unwrap_or_else(|e| panic!("{at}: {e}"));
                    match answer {
                        PostReturn::Post(object) => {
                            let got = from_hook_value(object.handle.as_ref().unwrap()).unwrap();
                            let var = step["as"].as_str().unwrap();
                            let want = kept(var).unwrap_or_else(|| panic!("{at}: kept {var}"));
                            if let Some(path) =
                                first_difference(&reported(got), &reported(want), String::new())
                            {
                                panic!("{at}: the post returned differs at {path}");
                            }
                            None
                        }
                        PostReturn::Value(value) => Some(value),
                    }
                }
                "wp_publish_post" => {
                    f.api.wp_publish_post(post_arg(0)).unwrap();
                    Some(PhpValue::Null)
                }
                "check_and_publish_future_post" => {
                    f.api.check_and_publish_future_post(post_arg(0)).unwrap();
                    Some(PhpValue::Null)
                }
                "stick_post" | "unstick_post" => {
                    let id = to_php_int(&arg(0).unwrap().value());
                    if step["call"] == "stick_post" {
                        f.api.stick_post(id).unwrap();
                    } else {
                        f.api.unstick_post(id).unwrap();
                    }
                    Some(PhpValue::Null)
                }
                "add_ping" => {
                    let rows = f
                        .api
                        .add_ping(post_arg(0), &arg(1).unwrap().value())
                        .unwrap();
                    Some(rows.map_or(PhpValue::Bool(false), |n| PhpValue::Int(n as i64)))
                }
                "wp_delete_auto_drafts" => {
                    f.api.wp_delete_auto_drafts().unwrap();
                    Some(PhpValue::Null)
                }
                "do_action" => {
                    let hook = args[0].as_str().unwrap();
                    let rest = (1..args.len())
                        .map(|i| speedyspec_php_serialize::to_hook_value(&arg(i).unwrap().value()))
                        .collect();
                    f.hooks
                        .do_action(&HookName::from(hook), rest)
                        .unwrap_or_else(|e| panic!("{at}: {e}"));
                    Some(PhpValue::Null)
                }
                // get_the_guid(): the post's `guid` as `get_post()` reads it (no `get_the_guid` filter is added).
                "get_the_guid" => {
                    let id = to_php_int(&arg(0).unwrap().value());
                    let post = f.posts.get_post(PostRef::Id(PostId(id as u64))).unwrap();
                    Some(s(&String::from_utf8_lossy(&post.unwrap().guid)))
                }
                "get_option" => Some(f.options.get(args[0].as_str().unwrap())),
                "is_sticky" => Some(f.posts.is_sticky(&arg(0).unwrap().value()).unwrap()),
                "get_pung" | "get_to_ping" | "get_post_status" => {
                    let post = match post_arg(0) {
                        PostArg::Ref(post) => post,
                        _ => unreachable!(),
                    };
                    Some(
                        match step["call"].as_str().unwrap() {
                            "get_pung" => f.posts.get_pung(post),
                            "get_to_ping" => f.posts.get_to_ping(post),
                            _ => f.posts.get_post_status(post),
                        }
                        .unwrap(),
                    )
                }
                // Term reads are the taxonomy code's (WordPress's until it is ported): not replayed.
                "wp_get_post_categories"
                | "wp_get_post_terms"
                | "wp_get_object_terms"
                | "get_term" => {
                    continue;
                }
                other => panic!("{at}: {other} is not replayed"),
            };
            if let Some(returned) = returned
                && result["returned"].get("callback").is_none()
            {
                assert_eq!(
                    returned,
                    support::decode(&result["returned"]).unwrap(),
                    "{at}: returned"
                );
            }
            let got: Vec<(String, PhpValue)> = f
                .log
                .borrow()
                .iter()
                .map(|(l, a)| (l.clone(), reported(a.clone())))
                .filter(|(_, a)| !from_revision(a))
                .collect();
            let want: Vec<_> = support::recorded_invocations(result)
                .into_iter()
                .filter(|(l, a)| !from_revision(a) && !FROM_UNPORTED.contains(&l.as_str()))
                .collect();
            let labels = |list: &[(String, PhpValue)]| {
                list.iter().map(|(l, _)| l.clone()).collect::<Vec<_>>()
            };
            assert_eq!(
                labels(&got),
                labels(&want),
                "{at}: the fixture callbacks' runs"
            );
            for ((label, got), (_, want)) in got.iter().zip(&want) {
                if let Some(path) = first_difference(got, want, String::new()) {
                    panic!("{at}: {label}'s arguments differ at {path}");
                }
            }
            let snapshot = result.get("snapshot");
            if let Some(rows) = snapshot.and_then(|s| snapshot_table(s, "wp_posts")) {
                // The revisions WordPress's revision code wrote during the step are copied in (it stays
                // WordPress's until phase 6), so later inserts take the ids they took there.
                copy_revisions(&f, &rows);
                let comparable = sorted;
                let want: Vec<_> = rows.into_iter().map(comparable).collect();
                let got: Vec<_> = table(&f).into_iter().map(comparable).collect();
                for (got, want) in got.iter().zip(&want) {
                    assert_eq!(got, want, "{at}: a wp_posts row");
                }
                assert_eq!(got.len(), want.len(), "{at}: wp_posts rows");
            }
            if let Some(rows) = snapshot.and_then(|s| snapshot_table(s, "wp_postmeta")) {
                // `_wp_old_date` and `_wp_old_slug` are written by `wp_check_for_changed_dates()` and
                // `wp_check_for_changed_slugs()` on `post_updated`, which stay WordPress's (default-filters.php
                // registers them), so the replay leaves them out.
                let want: Vec<_> = rows
                    .into_iter()
                    .filter(|r| !matches!(col(r, "meta_key"), "_wp_old_date" | "_wp_old_slug"))
                    .map(|r| {
                        (
                            col(&r, "post_id").to_owned(),
                            col(&r, "meta_key").to_owned(),
                            col(&r, "meta_value").to_owned(),
                        )
                    })
                    .collect();
                assert_eq!(meta_table(&f), want, "{at}: wp_postmeta");
            }
            if let Some(cron) = result.get("cron") {
                let mut got = f.cron.0.borrow().clone();
                got.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
                assert_eq!(got, recorded_cron(cron), "{at}: cron");
            }
        }
    });
}

#[test]
fn transitions_replay() {
    for name in [
        "transition-from-new",
        "transition-from-auto-draft",
        "transition-from-draft",
        "transition-from-pending",
        "transition-from-private",
        "transition-from-publish",
        "transition-from-future",
        "transition-from-trash",
        "transition-page-from-draft",
        "transition-memo-from-draft",
    ] {
        replay(name);
    }
}

#[test]
fn inserts_replay() {
    // Not replayed here: insert-import-id and insert-slug-trashed-suffix (their own tests below),
    // insert-parent-loop-broken (loop_is_broken_through_wp_update_post), and insert-tax-input and insert-kses, which
    // switch to a user without `unfiltered_html` and so run kses and the taxonomy code, both WordPress's.
    for name in [
        "insert-defaults",
        "insert-every-field",
        "insert-empty-content",
        "insert-parent-loop-self",
        "insert-parent-negative",
        "insert-parent-missing",
        "insert-slug-unique",
        "insert-slug-forced",
        "insert-slug-hierarchical",
        "insert-date-future",
        "insert-date-drafts",
        "insert-emoji",
        "insert-meta-input",
        "insert-page-template",
        "insert-hook-order",
        "insert-hook-order-no-after-hooks",
        "insert-unknown-status-and-type",
    ] {
        replay(name);
    }
}

#[test]
fn updates_replay() {
    for name in [
        "update-partial",
        "update-status",
        "update-date-clear-draft",
        "update-date-edit-date",
        "update-post-updated-arguments",
        "update-missing",
        "update-categories-keep",
    ] {
        replay(name);
    }
}

#[test]
fn set_post_type_replays() {
    replay("set-type");
}

#[test]
fn deletes_replay() {
    // Not replayed: delete-attachments, which goes to `wp_delete_attachment()` (the uploads port, phase 7).
    for name in [
        "delete-force",
        "delete-not-forced",
        "delete-children-reparented",
        "delete-front-page",
        "delete-revision",
        "delete-missing",
        "delete-hierarchical-book",
    ] {
        replay(name);
    }
}

#[test]
fn trash_and_untrash_replay() {
    for name in [
        "trash-with-comments",
        "trash-already-trashed",
        "trash-front-page",
        "trash-draft-no-slug",
        "untrash-default-status",
        "untrash-previous-status",
        "untrash-not-trashed",
        "trash-untrash-slug-taken",
        "trash-untrash-round-trip",
    ] {
        replay(name);
    }
}

#[test]
fn publishing_replays() {
    for name in [
        "publish-draft",
        "publish-future-early",
        "publish-already-and-missing",
        "future-scheduled-then-published",
        "future-too-early",
        "future-hook-publishes",
        "future-back-to-draft",
        "transition-guid-reset",
    ] {
        replay(name);
    }
}

#[test]
fn sticky_ping_and_auto_draft_replays() {
    for name in [
        "sticky-stick",
        "sticky-unstick",
        "sticky-insert",
        "ping-add",
        "ping-to-ping",
        "ping-pinged-removed",
        "auto-drafts-cleanup",
        "auto-drafts-insert",
    ] {
        replay(name);
    }
}

/// Review focus 1: a parent that closes a loop the post is not on. `wp_check_post_hierarchy_for_loops()` runs inside
/// the `wp_insert_post_parent` step and calls `wp_update_post( post_parent 0 )` on each member, in order, before the
/// post's own slug and row; the post keeps the parent it asked for.
#[test]
fn loop_is_broken_through_wp_update_post() {
    replay("insert-parent-loop-broken");
    let f = fixture();
    let order: Rc<RefCell<Vec<String>>> = Rc::default();
    let seen = order.clone();
    f.hooks
        .add_filter(
            &HookName::from("pre_post_update"),
            Callback::rust("order", move |args: &[HookValue]| {
                seen.borrow_mut().push(format!(
                    "update {}",
                    to_php_int(&from_hook_value(&args[0]).unwrap())
                ));
                Ok(HookValue::Null)
            }),
            &HookValue::Int(10),
            2,
        )
        .unwrap();
    let id = f.api.wp_update_post(
        array(vec![
            ("ID", PhpValue::Int(13)),
            ("post_parent", PhpValue::Int(9)),
        ]),
        false,
        true,
    );
    assert_eq!(id.unwrap(), Ok(PostId(13)));
    // The members, in wp_find_hierarchy_loop()'s order, then the post itself.
    assert_eq!(*order.borrow(), ["update 9", "update 10", "update 13"]);
    assert_eq!(f.row(9).parent, PostParent::Int(0));
    assert_eq!(f.row(10).parent, PostParent::Int(0));
    assert_eq!(f.row(13).parent, PostParent::Int(9));
}

/// `import_id` is honoured only when no row has it: 500 is free, 6 is taken (so the next id), 0 is empty.
#[test]
fn import_id_honoured_only_when_free() {
    replay("insert-import-id");
    let f = fixture();
    let first = f.insert(array(vec![
        ("post_title", s("Imported")),
        ("import_id", PhpValue::Int(500)),
    ]));
    assert_eq!(first, Ok(PostId(500)));
    let taken = f.insert(array(vec![
        ("post_title", s("Taken")),
        ("import_id", PhpValue::Int(6)),
    ]));
    assert_eq!(taken, Ok(PostId(501)));
    assert_eq!(f.row(6).title, b"Title 6".to_vec());
}

/// Review focus 2: a `save_post` callback that throws. The row is written, the exception comes back whole, and no
/// hook after it runs.
#[test]
fn hook_error_after_insert_keeps_the_row() {
    let f = fixture();
    let payload: Rc<dyn std::any::Any> = Rc::new("thrown by save_post");
    let kept = payload.clone();
    f.hooks
        .add_filter(
            &HookName::from("save_post"),
            Callback::rust("fail", move |_: &[HookValue]| {
                Err::<HookValue, _>(HookError {
                    message: "thrown".into(),
                    payload: Some(kept.clone()),
                })
            }),
            &HookValue::Int(10),
            3,
        )
        .unwrap();
    let after: Rc<Cell<bool>> = Rc::default();
    let ran = after.clone();
    f.hooks
        .add_filter(
            &HookName::from("wp_insert_post"),
            Callback::rust("after", move |_: &[HookValue]| {
                ran.set(true);
                Ok(HookValue::Null)
            }),
            &HookValue::Int(10),
            3,
        )
        .unwrap();
    let Err(PostError::Hook(e)) =
        f.api
            .wp_insert_post(array(vec![("post_title", s("Saved"))]), false, true)
    else {
        panic!("the callback's failure comes back");
    };
    assert!(Rc::ptr_eq(e.payload.as_ref().unwrap(), &payload));
    assert!(!after.get(), "nothing after save_post runs");
    let row = f.row(26);
    assert_eq!(row.title, b"Saved".to_vec());
    assert_eq!(
        row.guid,
        b"http://example.org/?p=26".to_vec(),
        "the GUID was written before the hook"
    );
}

/// A `query` callback's exception `$wpdb` hid behind an empty answer stops the use case where WordPress would have
/// thrown: the published post's slug update answers 0, and no hook after it runs.
#[test]
fn hidden_query_failure_stops_the_insert() {
    let f = fixture_with(|repo| Rc::new(HidingRepository(repo)));
    let _ = speedyspec_wp_db_contracts::take_hook_error();
    let saved: Rc<Cell<bool>> = Rc::default();
    let ran = saved.clone();
    f.hooks
        .add_filter(
            &HookName::from("save_post"),
            Callback::rust("saved", move |_: &[HookValue]| {
                ran.set(true);
                Ok(HookValue::Null)
            }),
            &HookValue::Int(10),
            3,
        )
        .unwrap();
    // A slug of only punctuation sanitizes to nothing, so wp_insert_post() writes the title's slug afterwards.
    let result = f.api.wp_insert_post(
        array(vec![
            ("post_title", s("!!!")),
            ("post_status", s("publish")),
            ("post_name", s("!!!")),
        ]),
        false,
        true,
    );
    let Err(PostError::Hook(e)) = result else {
        panic!("the hidden failure stops it: {result:?}")
    };
    assert_eq!(e.message, "query threw");
    assert!(!saved.get());
    assert!(
        speedyspec_wp_db_contracts::take_hook_error().is_some(),
        "it stays for the bridge to rethrow"
    );
}

/// Review focus 3: the `__trashed` suffix and `_wp_desired_post_slug` round-trip. A trashed post holding the wanted
/// slug gives it up (recorded), and a post trashed then restored takes its slug back.
#[test]
fn trashed_suffix_round_trips() {
    replay("insert-slug-trashed-suffix");
    let f = fixture();
    // Trash post 21 (`trashed-one`): it takes the suffix, and the desired slug is kept.
    let trashed = f.api.wp_update_post(
        array(vec![("ID", PhpValue::Int(21)), ("post_status", s("trash"))]),
        false,
        true,
    );
    assert_eq!(trashed.unwrap(), Ok(PostId(21)));
    // Post 7 already holds `trashed-one__trashed`, so wp_unique_post_slug() numbers it.
    assert_eq!(f.row(21).name.as_bytes(), b"trashed-one__trashed-2");
    assert_eq!(f.meta_of(21, "_wp_desired_post_slug"), ["trashed-one"]);
    // Restore it: the desired slug comes back and its meta goes.
    let restored = f.api.wp_update_post(
        array(vec![
            ("ID", PhpValue::Int(21)),
            ("post_status", s("publish")),
        ]),
        false,
        true,
    );
    assert_eq!(restored.unwrap(), Ok(PostId(21)));
    assert_eq!(f.row(21).name.as_bytes(), b"trashed-one");
    assert!(f.meta_of(21, "_wp_desired_post_slug").is_empty());
    // Restoring post 7 while 21 holds the slug: 7 takes it back numbered.
    let seven = f.api.wp_update_post(
        array(vec![("ID", PhpValue::Int(7)), ("post_status", s("draft"))]),
        false,
        true,
    );
    assert_eq!(seven.unwrap(), Ok(PostId(7)));
    assert_eq!(f.row(7).name.as_bytes(), b"trashed-one");
}

/// The flat branch's date-archive test reads `get_post( 0 )` for a new post, which is the global `$post` (the phase
/// 2b ruling): with a global post named like the slug, the numeric slug is its own and stays.
#[test]
fn new_post_slug_reads_the_global_post() {
    let f = fixture();
    *f.rewriting.0.borrow_mut() = b"/%postname%/".to_vec();
    let numeric = || {
        array(vec![
            ("post_title", s("Five")),
            ("post_name", s("5")),
            ("post_status", s("publish")),
        ])
    };
    assert_eq!(f.insert(numeric()), Ok(PostId(26)));
    assert_eq!(
        f.row(26).name.as_bytes(),
        b"5-2",
        "a number would read as a date archive"
    );
    let global = PostEntry::wp_post(PostEntry::raw_properties(&Post {
        id: PostId(6),
        name: Slug::from("5"),
        ..Post::default()
    }));
    *f.global.0.borrow_mut() = Some(global);
    assert_eq!(f.insert(numeric()), Ok(PostId(27)));
    assert_eq!(
        f.row(27).name.as_bytes(),
        b"5",
        "the global post's own name is no conflict"
    );
}

/// A `future` post schedules `publish_future_post` at its GMT date through `future_post`'s default callback, and
/// `_transition_post_status` clears it again when the post leaves `future`.
#[test]
fn future_post_is_scheduled_through_the_transition() {
    let f = fixture();
    let id = f.insert(array(vec![
        ("post_title", s("Later")),
        ("post_status", s("publish")),
        ("post_date", s("2027-03-01 10:00:00")),
    ]));
    assert_eq!(id, Ok(PostId(26)));
    assert_eq!(f.row(26).status.as_str(), "future");
    let args = PhpValue::Array(vec![(PhpKey::Int(0), PhpValue::Int(26))]);
    let at = parse(b"2027-03-01 08:00:00").unwrap();
    assert_eq!(
        *f.cron.0.borrow(),
        [(at, "publish_future_post".to_owned(), args.clone())]
    );
    f.api
        .wp_update_post(
            array(vec![("ID", PhpValue::Int(26)), ("post_status", s("draft"))]),
            false,
            true,
        )
        .unwrap()
        .unwrap();
    assert!(f.cron.0.borrow().is_empty());
}

/// `tax_input` naming a taxonomy that is not registered calls `_doing_it_wrong()` and goes on.
#[test]
fn unknown_taxonomy_is_doing_it_wrong() {
    let f = fixture();
    let tax = array(vec![
        ("nope", PhpValue::Array(vec![(PhpKey::Int(0), s("x"))])),
        ("post_tag", s("a, b")),
    ]);
    f.insert(array(vec![("post_title", s("Taxed")), ("tax_input", tax)]))
        .unwrap();
    assert_eq!(
        *f.runtime.0.borrow(),
        ["wp_insert_post: Invalid taxonomy: nope. (4.4.0)"]
    );
    let set = f.terms.set.borrow();
    assert!(set.iter().any(|c| c.starts_with("26:category:")));
    assert!(set.iter().any(|c| c == "26:post_tag:s:4:\"a, b\";:false"));
}

/// The default callbacks carry WordPress's function names, so they can be removed as WordPress's are.
#[test]
fn default_callbacks_are_removable_by_name() {
    let f = fixture();
    let removed = f
        .hooks
        .remove_filter(
            &HookName::from("transition_post_status"),
            speedyspec_wp_hooks_contracts::CallbackId(Some("_transition_post_status".into())),
            &HookValue::Int(5),
        )
        .unwrap();
    assert!(removed);
    let post = f
        .posts
        .get_post(speedyspec_wp_posts_contracts::PostRef::Id(PostId(15)))
        .unwrap()
        .unwrap();
    f.api
        .wp_transition_post_status(
            &PostStatusName::new("draft"),
            &PostStatusName::new("future"),
            &PostObject::new(post),
        )
        .unwrap();
    // `_transition_post_status` would have cleared nothing here anyway; `future_post` did not run either.
    assert!(f.cron.0.borrow().is_empty());
}

/// Every write message is bound under `SpeedySpec\WpPosts\<Name>Handler`, and the bus runs the use case.
#[test]
fn write_handlers_are_bound() {
    use speedyspec_cqrs::Command;
    use speedyspec_wp_posts::{
        CheckPostHierarchyForLoops, FuturePostHook, PublishPostHook, SetPostType,
        TransitionPostStatusDefault, WpInsertPost, WpTransitionPostStatus, WpUpdatePost,
        register_post_write_handlers,
    };
    let f = fixture();
    let container = Rc::new(speedyspec_container::Container::new());
    register_post_write_handlers(&container, f.api.clone());
    for key in [
        <WpInsertPost as Command>::KEY,
        <WpUpdatePost as Command>::KEY,
        <WpTransitionPostStatus as Command>::KEY,
        <SetPostType as Command>::KEY,
        <TransitionPostStatusDefault as Command>::KEY,
        <PublishPostHook as Command>::KEY,
        <FuturePostHook as Command>::KEY,
        <CheckPostHierarchyForLoops as Command>::KEY,
    ] {
        assert!(container.has(key), "{key}");
    }
    assert_eq!(
        <WpInsertPost as Command>::KEY,
        "SpeedySpec\\WpPosts\\WpInsertPostHandler"
    );
    let bus = speedyspec_cqrs::CommandBus::new(container.clone());
    let id = bus
        .dispatch(WpInsertPost {
            postarr: array(vec![("post_title", s("Bus"))]),
            wp_error: false,
            fire_after_hooks: true,
        })
        .unwrap()
        .unwrap();
    assert_eq!(id, Ok(PostId(26)));
    assert_eq!(f.row(26).title, b"Bus".to_vec());
}

// ---- fix round 1 ----

/// Adds a `wp_insert_post_data` callback answering `answer( $data )`.
fn on_data(f: &Fixture, answer: impl Fn(PhpValue) -> PhpValue + 'static) {
    f.hooks
        .add_filter(
            &HookName::from("wp_insert_post_data"),
            Callback::rust("data", move |args: &[HookValue]| {
                Ok(speedyspec_php_serialize::to_hook_value(&answer(
                    from_hook_value(&args[0]).unwrap(),
                )))
            }),
            &HookValue::Int(10),
            4,
        )
        .unwrap();
}

/// Counts the runs of `hook`.
fn count_runs(f: &Fixture, hook: &str) -> Rc<Cell<u32>> {
    let runs: Rc<Cell<u32>> = Rc::default();
    let seen = runs.clone();
    f.hooks
        .add_filter(
            &HookName::from(hook),
            Callback::rust("count", move |args: &[HookValue]| {
                seen.set(seen.get() + 1);
                Ok(args.first().cloned().unwrap_or(HookValue::Null))
            }),
            &HookValue::Int(10),
            1,
        )
        .unwrap();
    runs
}

fn unset_key(data: PhpValue, name: &str) -> PhpValue {
    let PhpValue::Array(mut items) = data else {
        panic!("$data is an array")
    };
    items.retain(|(k, _)| *k != k_(name));
    PhpValue::Array(items)
}

fn k_(name: &str) -> PhpKey {
    PhpKey::Str(name.as_bytes().to_vec())
}

fn thrown(result: Result<WriteResult, PostError>) -> speedyspec_wp_posts::PhpThrow {
    let error = result.expect_err("PHP throws");
    speedyspec_wp_posts::php_throw(&error)
        .unwrap_or_else(|| panic!("a PHP error, not {error:?}"))
        .clone()
}

/// A `wp_insert_post_data` callback that unsets `post_content` on an update leaves the stored content alone, as
/// `$wpdb->update` writes only `$data`'s columns.
#[test]
fn update_keeps_a_column_the_filter_dropped() {
    let f = fixture();
    on_data(&f, |data| unset_key(data, "post_content"));
    let id = f.api.wp_update_post(
        array(vec![
            ("ID", PhpValue::Int(6)),
            ("post_title", s("Retitled")),
            ("post_content", s("New body")),
        ]),
        true,
        true,
    );
    assert_eq!(id.unwrap(), Ok(PostId(6)));
    assert_eq!(f.row(6).content, b"Content 6".to_vec());
    assert_eq!(f.row(6).title, b"Retitled".to_vec());
}

/// An insert leaves a dropped column to its default and writes an added one; an unknown added column makes the
/// statement fail, which is `db_insert_error`.
#[test]
fn insert_writes_exactly_the_filtered_data() {
    let f = fixture();
    on_data(&f, |data| {
        let PhpValue::Array(mut items) = unset_key(data, "post_status") else {
            unreachable!()
        };
        items.push((k_("comment_count"), PhpValue::Int(4)));
        PhpValue::Array(items)
    });
    assert_eq!(
        f.insert(array(vec![("post_title", s("Filtered"))])),
        Ok(PostId(26))
    );
    let row = f.row(26);
    assert_eq!(row.status.as_str(), "publish", "the column's default");
    assert_eq!(row.comment_count, 4);

    let f = fixture();
    on_data(&f, |data| {
        let PhpValue::Array(mut items) = data else {
            unreachable!()
        };
        items.push((k_("bogus"), s("x")));
        PhpValue::Array(items)
    });
    let refused = f.insert(array(vec![("post_title", s("Unknown"))]));
    // The WP_Error's data is `$wpdb->last_error`, as the repository reported it.
    assert_eq!(
        refused,
        Err(PostWriteError::DbInsert {
            attachment: false,
            last_error: "Unknown column 'bogus' in 'field list'".into(),
        })
    );
    assert!(f.repo.find(PostId(26)).unwrap().is_none());
}

/// A filter answer that is not an array: an update is `db_update_error` with no statement; an insert of `null` with
/// a free `import_id` writes `ID` alone; any other insert raises PHP's `TypeError` after `pre_post_insert`.
#[test]
fn non_array_data_follows_wordpress() {
    let f = fixture();
    on_data(&f, |_| PhpValue::Bool(false));
    let before = f.row(6);
    let refused = f
        .api
        .wp_update_post(
            array(vec![("ID", PhpValue::Int(6)), ("post_title", s("Never"))]),
            true,
            true,
        )
        .unwrap();
    assert_eq!(refused.map_err(|e| e.code()), Err(Some("db_update_error")));
    assert_eq!(f.row(6), before);

    let f = fixture();
    on_data(&f, |_| PhpValue::Null);
    let id = f.insert(array(vec![
        ("post_title", s("Only an id")),
        ("import_id", PhpValue::Int(900)),
    ]));
    assert_eq!(id, Ok(PostId(900)));
    let row = f.row(900);
    assert_eq!(
        row.title,
        Vec::<u8>::new(),
        "every other column is the default"
    );
    assert_eq!(row.status.as_str(), "publish");

    let f = fixture();
    on_data(&f, |_| s("not an array"));
    let inserts = count_runs(&f, "pre_post_insert");
    let error = thrown(
        f.api
            .wp_insert_post(array(vec![("post_title", s("Typed"))]), true, true),
    );
    assert_eq!(error.class, "TypeError");
    assert_eq!(
        error.message,
        "array_keys(): Argument #1 ($array) must be of type array, string given"
    );
    assert_eq!(
        inserts.get(),
        1,
        "pre_post_insert ran before the insert threw"
    );
    assert!(f.repo.find(PostId(26)).unwrap().is_none());

    let f = fixture();
    on_data(&f, |_| s("x"));
    let error = thrown(f.api.wp_insert_post(
        array(vec![
            ("post_title", s("Typed")),
            ("import_id", PhpValue::Int(901)),
        ]),
        true,
        true,
    ));
    assert_eq!(
        error.message,
        "Cannot access offset of type string on string"
    );
}

/// A scalar or `null` `$postarr` fails as WordPress's does: a string at `$postarr['ID']`; any other value once the
/// global post is read, at `array_merge()`. Nothing is written and no write hook runs. With no global post, it is
/// `invalid_post`.
#[test]
fn scalar_postarr_to_wp_update_post_throws() {
    let f = fixture();
    let error = thrown(f.api.wp_update_post(s("ID=6"), false, true));
    assert_eq!(
        (error.class, error.message.as_str()),
        ("TypeError", "Cannot access offset of type string on string")
    );

    let missing = f.api.wp_update_post(PhpValue::Int(5), true, true).unwrap();
    assert_eq!(missing.map_err(|e| e.code()), Err(Some("invalid_post")));

    let global = PostEntry::wp_post(PostEntry::raw_properties(&f.row(6)));
    *f.global.0.borrow_mut() = Some(global);
    let updates = count_runs(&f, "pre_post_update");
    let before = f.row(6);
    for (value, given) in [
        (PhpValue::Int(5), "int"),
        (PhpValue::Null, "null"),
        (PhpValue::Bool(true), "true"),
    ] {
        let error = thrown(f.api.wp_update_post(value, false, true));
        assert_eq!(
            error.message,
            format!("array_merge(): Argument #2 must be of type array, {given} given")
        );
    }
    assert_eq!(updates.get(), 0);
    assert_eq!(f.row(6), before);
}

/// A scalar `post_category` reaches `array_filter()`, which throws: no row is written.
#[test]
fn scalar_post_category_throws() {
    let f = fixture();
    let error = thrown(f.api.wp_insert_post(
        array(vec![("post_title", s("Cats")), ("post_category", s("3"))]),
        true,
        true,
    ));
    assert_eq!(
        error.message,
        "array_filter(): Argument #1 ($array) must be of type array, string given"
    );
    assert!(f.repo.find(PostId(26)).unwrap().is_none());
}

/// A bad-slug callback that throws stops the use case before the `wp_unique_post_slug` filter and before any write.
#[test]
fn bad_slug_exception_stops_before_the_slug_filter() {
    let f = fixture();
    let payload: Rc<dyn std::any::Any> = Rc::new("bad slug");
    let kept = payload.clone();
    f.hooks
        .add_filter(
            &HookName::from("wp_unique_post_slug_is_bad_flat_slug"),
            Callback::rust("fail", move |_: &[HookValue]| {
                Err::<HookValue, _>(HookError {
                    message: "thrown".into(),
                    payload: Some(kept.clone()),
                })
            }),
            &HookValue::Int(10),
            3,
        )
        .unwrap();
    let slug_filter = count_runs(&f, "wp_unique_post_slug");
    let result = f.api.wp_insert_post(
        array(vec![
            ("post_title", s("Fresh")),
            ("post_status", s("publish")),
        ]),
        true,
        true,
    );
    let Err(PostError::Hook(e)) = result else {
        panic!("the callback's failure comes back: {result:?}")
    };
    assert!(Rc::ptr_eq(e.payload.as_ref().unwrap(), &payload));
    assert_eq!(slug_filter.get(), 0);
    assert!(f.repo.find(PostId(26)).unwrap().is_none());
}

/// `_transition_post_status`: a post first published with no GUID takes its permalink; another transition leaves the
/// empty GUID alone.
#[test]
fn transition_to_publish_resets_an_empty_guid() {
    let f = fixture();
    let post = f
        .posts
        .get_post(speedyspec_wp_posts_contracts::PostRef::Id(PostId(23)))
        .unwrap()
        .unwrap();
    assert!(post.guid.is_empty());
    let object = PostObject::new(post);
    f.api
        .wp_transition_post_status(
            &PostStatusName::new("pending"),
            &PostStatusName::new("draft"),
            &object,
        )
        .unwrap();
    assert!(f.row(23).guid.is_empty(), "only a first publish sets it");
    f.api
        .wp_transition_post_status(
            &PostStatusName::new("publish"),
            &PostStatusName::new("draft"),
            &object,
        )
        .unwrap();
    assert_eq!(f.row(23).guid, b"http://example.org/?p=23".to_vec());
    // A `get_the_guid` filter that answers a GUID keeps the column empty.
    let f = fixture();
    f.hooks
        .add_filter(
            &HookName::from("get_the_guid"),
            Callback::rust("guid", |_: &[HookValue]| {
                Ok(HookValue::Str("urn:kept".into()))
            }),
            &HookValue::Int(10),
            2,
        )
        .unwrap();
    let post = f
        .posts
        .get_post(speedyspec_wp_posts_contracts::PostRef::Id(PostId(23)))
        .unwrap()
        .unwrap();
    f.api
        .wp_transition_post_status(
            &PostStatusName::new("publish"),
            &PostStatusName::new("draft"),
            &PostObject::new(post),
        )
        .unwrap();
    assert!(f.row(23).guid.is_empty());
}

// ---- fix round 2 ----

/// A clock that throws (an option filter while it reads the time zone) stops `wp_insert_post()` where WordPress's
/// exception would: no row, no hook after it.
#[test]
fn throwing_clock_stops_the_insert() {
    let f = fixture();
    let filtered = count_runs(&f, "wp_insert_post_data");
    CLOCK_THROWS.with(|c| c.set(true));
    let result = f
        .api
        .wp_insert_post(array(vec![("post_title", s("Clocked"))]), true, true);
    CLOCK_THROWS.with(|c| c.set(false));
    let Err(PostError::Hook(e)) = result else {
        panic!("the clock's failure comes back: {result:?}")
    };
    assert_eq!(e.message, "option_gmt_offset threw");
    assert_eq!(filtered.get(), 0);
    assert!(f.repo.find(PostId(26)).unwrap().is_none());
}

/// A float, a bool or an array a `wp_insert_post_data` callback puts in `$data` reaches the repository as the PHP
/// value it is, which formats it as `wpdb` does: a float under `%s` is its `(string)`, `true` is `'1'`, an array is
/// `''`, and a float under `%d` is truncated.
#[test]
fn filtered_php_values_are_formatted_by_the_repository() {
    let f = fixture();
    on_data(&f, |data| {
        let PhpValue::Array(mut items) = data else {
            unreachable!()
        };
        for (key, value) in &mut items {
            match key_to_string(key).as_str() {
                "post_content" => *value = PhpValue::Float(1.5),
                "post_excerpt" => *value = PhpValue::Bool(true),
                "post_password" => *value = PhpValue::Array(Vec::new()),
                "menu_order" => *value = PhpValue::Float(2.9),
                _ => {}
            }
        }
        PhpValue::Array(items)
    });
    assert_eq!(
        f.insert(array(vec![("post_title", s("Typed"))])),
        Ok(PostId(26))
    );
    let row = f.row(26);
    assert_eq!(row.content, b"1.5".to_vec());
    assert_eq!(row.excerpt, b"1".to_vec());
    assert!(row.password.is_empty());
    assert_eq!(row.menu_order, 2);
}

// ---- fix round 3 ----

/// A hook failure pending from earlier (raised and surfaced before) does not fail a later write that changed
/// nothing: only a failure raised during the write stops it, and the earlier one stays pending.
#[test]
fn a_stale_hook_error_does_not_fail_a_write() {
    let f = fixture();
    let _ = speedyspec_wp_db_contracts::take_hook_error();
    let _: speedyspec_wp_db_contracts::WpDbError = HookError::message("earlier").into();
    // Post 21 is already a `post`, so the update changes no row.
    let id = f
        .api
        .set_post_type(21, &s("post"))
        .expect("an unchanged write is not failed by an earlier error");
    assert_eq!(id, Some(0));
    assert_eq!(
        speedyspec_wp_db_contracts::take_hook_error().map(|e| e.message),
        Some("earlier".to_owned())
    );
}

/// With an older failure still pending, a write's own failure is the one left in the side channel for the bridge: the
/// error the use case returns, not the older one.
#[test]
fn a_new_hook_error_wins_over_a_stale_one() {
    let f = fixture_with(|repo| Rc::new(HidingRepository(repo)));
    let _ = speedyspec_wp_db_contracts::take_hook_error();
    let _: speedyspec_wp_db_contracts::WpDbError = HookError::message("earlier").into();
    let result = f.api.wp_insert_post(
        array(vec![
            ("post_title", s("!!!")),
            ("post_status", s("publish")),
            ("post_name", s("!!!")),
        ]),
        false,
        true,
    );
    let Err(PostError::Hook(e)) = result else {
        panic!("the hidden failure stops it: {result:?}")
    };
    assert_eq!(e.message, "query threw");
    assert_eq!(
        speedyspec_wp_db_contracts::take_hook_error().map(|e| e.message),
        Some("query threw".to_owned())
    );
}

/// `_transition_post_status` reads `get_the_guid( $post->ID )`, which falls back to the global `$post` for ID 0.
#[test]
fn guid_of_id_zero_reads_the_global_post() {
    let f = fixture();
    *f.global.0.borrow_mut() = Some(PostEntry::wp_post(PostEntry::raw_properties(&f.row(23))));
    let seen: Rc<RefCell<Vec<PhpValue>>> = Rc::default();
    let log = seen.clone();
    f.hooks
        .add_filter(
            &HookName::from("get_the_guid"),
            Callback::rust("guid", move |args: &[HookValue]| {
                log.borrow_mut().push(from_hook_value(&args[1]).unwrap());
                Ok(args[0].clone())
            }),
            &HookValue::Int(10),
            2,
        )
        .unwrap();
    f.api
        .transition_post_status_default("publish", "draft", PostId(0), "post")
        .unwrap();
    assert_eq!(*seen.borrow(), [PhpValue::Int(23)]);
}

// ---- phase 5b: delete, trash, publish, sticky posts, pings and auto-drafts ----

/// A callback on `hook` that records the `(int)` of each run's first argument.
fn record_ids(f: &Fixture, hook: &str) -> Rc<RefCell<Vec<i64>>> {
    let ids: Rc<RefCell<Vec<i64>>> = Rc::default();
    let seen = ids.clone();
    f.hooks
        .add_filter(
            &HookName::from(hook),
            Callback::rust("record", move |args: &[HookValue]| {
                seen.borrow_mut()
                    .push(to_php_int(&from_hook_value(&args[0]).unwrap()));
                Ok(args[0].clone())
            }),
            &HookValue::Int(10),
            2,
        )
        .unwrap();
    ids
}

fn by_id(id: u64) -> PostArg {
    PostArg::Ref(PostRef::Id(PostId(id)))
}

/// Review focus 4: a future post is scheduled through the cron contracts when it is written, rescheduled by
/// `check_and_publish_future_post()` while its time has not come, and published by it (directly and through the
/// `publish_future_post` hook) once it has: the event is cleared and `_publish_post_hook()` schedules `do_pings`.
#[test]
fn future_post_is_scheduled_and_published() {
    let f = fixture();
    let id = f.insert(array(vec![
        ("post_title", s("Scheduled")),
        ("post_status", s("publish")),
        ("post_date", s("2027-06-01 10:00:00")),
    ]));
    assert_eq!(id, Ok(PostId(26)));
    assert_eq!(f.row(26).status.as_str(), "future");
    let at = parse(b"2027-06-01 08:00:00").unwrap();
    let args = |id: i64| PhpValue::Array(vec![(PhpKey::Int(0), PhpValue::Int(id))]);
    assert_eq!(
        *f.cron.0.borrow(),
        [(at, "publish_future_post".to_owned(), args(26))]
    );

    // Too early: cleared and scheduled again at the same time, still future.
    f.cron.0.borrow_mut().clear();
    f.api.check_and_publish_future_post(by_id(26)).unwrap();
    assert_eq!(f.row(26).status.as_str(), "future");
    assert_eq!(
        *f.cron.0.borrow(),
        [(at, "publish_future_post".to_owned(), args(26))]
    );

    // Not future, or no post: nothing.
    f.api.check_and_publish_future_post(by_id(6)).unwrap();
    f.api.check_and_publish_future_post(by_id(999)).unwrap();
    assert_eq!(f.cron.0.borrow().len(), 1);

    // Its time has come: published through the transition, the event cleared, `do_pings` scheduled now.
    CLOCK.with(|c| c.set(at + 3600));
    let transitions = record_ids(&f, "future_to_publish");
    f.api.check_and_publish_future_post(by_id(26)).unwrap();
    assert_eq!(f.row(26).status.as_str(), "publish");
    assert_eq!(transitions.borrow().len(), 1);
    assert_eq!(
        *f.cron.0.borrow(),
        [(
            at + 3600,
            "do_pings".to_owned(),
            PhpValue::Array(Vec::new())
        )]
    );

    // The seeded future post 15, through the hook default-filters.php registers.
    f.hooks
        .do_action(
            &HookName::from("publish_future_post"),
            vec![HookValue::Int(15)],
        )
        .unwrap();
    assert_eq!(f.row(15).status.as_str(), "publish");
    CLOCK.with(|c| c.set(NOW));
}

/// Deleting a hierarchical post points its children at its own parent, then cleans the post's cache (with the object,
/// the row being gone) and each child's, and unschedules `publish_future_post`; the comments are deleted between
/// deferring and flushing the counts.
#[test]
fn delete_reparents_children_and_cleans_caches() {
    let f = fixture();
    // Read them first, so their `posts` entries are cached.
    for id in [2, 3, 20] {
        f.posts.get_post(PostRef::Id(PostId(id))).unwrap().unwrap();
    }
    f.cron.0.borrow_mut().push((
        NOW + 60,
        "publish_future_post".to_owned(),
        PhpValue::Array(vec![(PhpKey::Int(0), PhpValue::Int(2))]),
    ));
    let cleaned = record_ids(&f, "clean_post_cache");
    let answer = f.api.wp_delete_post(2, true).unwrap();
    let PostReturn::Post(post) = answer else {
        panic!("the post")
    };
    assert_eq!(post.post.id, PostId(2));
    assert!(post.handle.is_some());
    assert!(f.repo.find(PostId(2)).unwrap().is_none());
    for child in [3, 20] {
        assert_eq!(f.row(child).parent, PostParent::Int(0), "child {child}");
        // The cache was cleaned: a read sees the new parent.
        let read = f
            .posts
            .get_post(PostRef::Id(PostId(child)))
            .unwrap()
            .unwrap();
        assert_eq!(read.parent, PostParent::Int(0));
    }
    assert_eq!(*cleaned.borrow(), [2, 3, 20]);
    assert!(f.cron.0.borrow().is_empty());
    assert_eq!(
        *f.comments.calls.borrow(),
        ["defer true", "delete 2", "defer false"]
    );
    // The trash meta of page 20 is its own; page 2 had none, and its rows are gone.
    assert!(f.meta_of(2, "_wp_trash_meta_status").is_empty());
}

/// Review focus 3: trashing gives the post the `__trashed` suffix and keeps its slug in `_wp_desired_post_slug`;
/// another post takes the slug meanwhile, so untrashing reclaims it as `taken-2` and drops the meta.
#[test]
fn trash_and_untrash_when_the_slug_is_taken() {
    let f = fixture();
    let PostReturn::Post(trashed) = f.api.wp_trash_post(by_id(6)).unwrap() else {
        panic!("the post")
    };
    assert_eq!(trashed.post.status.as_str(), "publish", "as it was read");
    assert_eq!(f.row(6).status.as_str(), "trash");
    assert_eq!(f.row(6).name.as_bytes(), b"taken__trashed");
    assert_eq!(f.meta_of(6, "_wp_desired_post_slug"), ["taken"]);
    assert_eq!(f.meta_of(6, "_wp_trash_meta_status"), ["publish"]);
    assert_eq!(f.meta_of(6, "_wp_trash_meta_time"), [NOW.to_string()]);

    let taker = f.insert(array(vec![
        ("post_title", s("Taken again")),
        ("post_name", s("taken")),
        ("post_status", s("publish")),
    ]));
    let taker = taker.unwrap().0;
    assert_eq!(f.row(taker).name.as_bytes(), b"taken");

    // wp_untrash_post_set_previous_status(): back to `publish`, so the slug goes through wp_unique_post_slug().
    f.hooks
        .add_filter(
            &HookName::from("wp_untrash_post_status"),
            Callback::rust(
                "wp_untrash_post_set_previous_status",
                |args: &[HookValue]| Ok(args[2].clone()),
            ),
            &HookValue::Int(10),
            3,
        )
        .unwrap();
    let PostReturn::Post(_) = f.api.wp_untrash_post(by_id(6)).unwrap() else {
        panic!("the post")
    };
    assert_eq!(f.row(6).status.as_str(), "publish");
    assert_eq!(f.row(6).name.as_bytes(), b"taken-2");
    assert_eq!(f.row(taker).name.as_bytes(), b"taken", "the taker keeps it");
    assert!(f.meta_of(6, "_wp_desired_post_slug").is_empty());
    assert!(f.meta_of(6, "_wp_trash_meta_status").is_empty());
    assert!(f.meta_of(6, "_wp_trash_meta_time").is_empty());
}

/// `pre_delete_post`, `pre_trash_post` and `pre_untrash_post` answer instead of the use case when not null, and
/// nothing is written.
#[test]
fn pre_filters_short_circuit() {
    let f = fixture();
    for hook in ["pre_delete_post", "pre_trash_post", "pre_untrash_post"] {
        f.hooks
            .add_filter(
                &HookName::from(hook),
                Callback::rust("refuse", |_: &[HookValue]| Ok(HookValue::Str("no".into()))),
                &HookValue::Int(10),
                1,
            )
            .unwrap();
    }
    assert_eq!(
        f.api.wp_delete_post(1, true).unwrap(),
        PostReturn::Value(s("no"))
    );
    assert_eq!(
        f.api.wp_trash_post(by_id(1)).unwrap(),
        PostReturn::Value(s("no"))
    );
    assert_eq!(
        f.api.wp_untrash_post(by_id(7)).unwrap(),
        PostReturn::Value(s("no"))
    );
    assert_eq!(f.row(1).status.as_str(), "publish");
    assert_eq!(f.row(7).status.as_str(), "trash");
}

/// An id of 0 or below is `_doing_it_wrong()` and `false`; with `EMPTY_TRASH_DAYS` 0, trashing deletes.
#[test]
fn delete_guards_and_disabled_trash() {
    let f = fixture();
    assert_eq!(
        f.api.wp_delete_post(0, false).unwrap(),
        PostReturn::Value(PhpValue::Bool(false))
    );
    assert_eq!(
        *f.runtime.0.borrow(),
        ["wp_delete_post: The post ID must be greater than 0. (6.9.0)"]
    );
    TRASH_DAYS.with(|d| d.set(0));
    let answer = f.api.wp_trash_post(by_id(6));
    TRASH_DAYS.with(|d| d.set(30));
    assert!(matches!(answer.unwrap(), PostReturn::Post(_)));
    assert!(f.repo.find(PostId(6)).unwrap().is_none());
}

/// A callback's exception stops the delete where it is thrown: the row stays, and nothing after runs.
#[test]
fn hook_error_stops_the_delete() {
    let f = fixture();
    f.hooks
        .add_filter(
            &HookName::from("delete_post"),
            Callback::rust("fail", |_: &[HookValue]| {
                Err::<HookValue, _>(HookError::message("thrown by delete_post"))
            }),
            &HookValue::Int(10),
            2,
        )
        .unwrap();
    let after = count_runs(&f, "after_delete_post");
    let Err(PostError::Hook(e)) = f.api.wp_delete_post(6, true) else {
        panic!("the hook error")
    };
    assert_eq!(e.message, "thrown by delete_post");
    assert!(f.repo.find(PostId(6)).unwrap().is_some());
    assert_eq!(after.get(), 0);
}

/// `wp_publish_post()` given an object reads its status, sets `post_status` to `publish` on it and fires the hooks
/// with that one object, which the caller gets back changed (the binding writes through its handle).
#[test]
fn publish_sets_the_status_on_the_given_object() {
    let f = fixture();
    let draft = f.row(19);
    let handle = speedyspec_wp_posts::PostObjects::handle(
        &PlainPostObjects,
        &PostObject::new(draft.clone()),
    )
    .unwrap();
    let statuses: Rc<RefCell<Vec<PhpValue>>> = Rc::default();
    let seen = statuses.clone();
    f.hooks
        .add_filter(
            &HookName::from("save_post"),
            Callback::rust("status", move |args: &[HookValue]| {
                let PhpValue::Object { properties, .. } = from_hook_value(&args[1]).unwrap() else {
                    panic!("an object")
                };
                seen.borrow_mut().push(
                    speedyspec_php_serialize::semantics::get(&properties, "post_status")
                        .cloned()
                        .unwrap(),
                );
                Ok(HookValue::Null)
            }),
            &HookValue::Int(10),
            3,
        )
        .unwrap();
    let transitions = record_ids(&f, "pending_to_publish");
    f.api
        .wp_publish_post(PostArg::Object(Box::new(PostObject::with_handle(
            draft, handle,
        ))))
        .unwrap();
    assert_eq!(f.row(19).status.as_str(), "publish");
    assert_eq!(*statuses.borrow(), [s("publish")]);
    assert_eq!(transitions.borrow().len(), 1);
}

/// `wp_publish_post()` gives a post with no category the default one, and leaves a published post alone.
#[test]
fn publish_assigns_the_default_category() {
    let f = fixture();
    f.api.wp_publish_post(by_id(8)).unwrap();
    assert_eq!(*f.terms.set.borrow(), ["8:category:a:1:{i:0;i:1;}:false"]);
    f.api.wp_publish_post(by_id(6)).unwrap();
    assert_eq!(f.terms.set.borrow().len(), 1);
}

/// `stick_post()` and `unstick_post()` keep `sticky_posts` a list of unique ints and fire `post_stuck` and
/// `post_unstuck` only when the option was written.
#[test]
fn sticky_posts_follow_wordpress() {
    let f = fixture();
    let stuck = count_runs(&f, "post_stuck");
    let unstuck = count_runs(&f, "post_unstuck");
    f.api.stick_post(6).unwrap();
    f.api.stick_post(6).unwrap();
    let list = |ids: &[i64]| {
        PhpValue::Array(
            ids.iter()
                .enumerate()
                .map(|(i, n)| (PhpKey::Int(i as i64), PhpValue::Int(*n)))
                .collect(),
        )
    };
    assert_eq!(f.options.get("sticky_posts"), list(&[1, 6]));
    assert_eq!(stuck.get(), 1);
    f.api.unstick_post(1).unwrap();
    f.api.unstick_post(999).unwrap();
    assert_eq!(f.options.get("sticky_posts"), list(&[6]));
    assert_eq!(unstuck.get(), 1);
}

/// Trashing the front page resets `show_on_front` and `page_on_front`; trashing a sticky post unsticks it
/// (`_reset_front_page_settings_for_post()` on `wp_trash_post`).
#[test]
fn trashing_resets_the_front_page_and_unsticks() {
    let f = fixture();
    f.api.wp_trash_post(by_id(3)).unwrap();
    assert_eq!(f.options.get("show_on_front"), s("posts"));
    assert_eq!(to_php_int(&f.options.get("page_on_front")), 0);
    f.api.wp_trash_post(by_id(1)).unwrap();
    assert_eq!(f.options.get("sticky_posts"), PhpValue::Array(Vec::new()));
}

/// `wp_delete_auto_drafts()` force-deletes the auto-drafts more than a week old (16, dated 2020) and keeps the rest
/// (17, dated 2099).
#[test]
fn old_auto_drafts_are_deleted() {
    let f = fixture();
    let deleted = record_ids(&f, "deleted_post");
    f.api.wp_delete_auto_drafts().unwrap();
    assert_eq!(*deleted.borrow(), [16]);
    assert!(f.repo.find(PostId(17)).unwrap().is_some());
}

/// `wp_delete_post()` deletes each revision through the revision port, before the attachments are re-pointed.
#[test]
fn revisions_are_deleted_through_the_port() {
    let f = fixture();
    f.api.wp_delete_post(1, true).unwrap();
    assert_eq!(*f.revisions.1.borrow(), [4]);
    assert!(f.repo.find(PostId(4)).unwrap().is_none());
    assert_eq!(
        f.row(5).parent,
        PostParent::Int(0),
        "the attachment is re-pointed"
    );
}

/// `add_ping()` appends to `pinged` through the `add_ping` filter and answers the rows changed; no post is false.
#[test]
fn pings_are_added() {
    let f = fixture();
    assert_eq!(
        f.api.add_ping(by_id(6), &s("http://a.example/")).unwrap(),
        Some(1)
    );
    // `preg_split()` of an empty `pinged` is one empty piece, so the first URL follows a line feed.
    assert_eq!(f.row(6).pinged, b"\nhttp://a.example/".to_vec());
    let both = PhpValue::Array(vec![
        (PhpKey::Int(0), s("http://b.example/")),
        (PhpKey::Int(1), s("http://c.example/")),
    ]);
    assert_eq!(f.api.add_ping(by_id(6), &both).unwrap(), Some(1));
    assert_eq!(
        f.row(6).pinged,
        b"http://a.example/\nhttp://b.example/\nhttp://c.example/".to_vec(),
        "trim() drops it the next time"
    );
    assert_eq!(f.api.add_ping(by_id(999), &s("x")).unwrap(), None);
}

/// The rest of the write contract: every method reaches its use case, and every message is bound.
#[test]
fn the_write_contract_and_messages() {
    use speedyspec_cqrs::Command;
    use speedyspec_wp_posts::{
        AddPing, CheckAndPublishFuturePost, ResetFrontPageSettingsForPost,
        ResetPrivacyPolicyPageForPost, StickPost, UnstickPost, WpDeleteAutoDrafts, WpDeletePost,
        WpPublishPost, WpTrashPost, WpUntrashPost, register_post_write_handlers,
    };
    use speedyspec_wp_posts_contracts::PostWrites;
    let f = fixture();
    let writes: &dyn PostWrites = &*f.api;
    writes.stick(8).unwrap();
    writes.unstick(8).unwrap();
    assert!(matches!(
        writes.trash(by_id(6)).unwrap(),
        PostReturn::Post(_)
    ));
    assert!(matches!(
        writes.untrash(by_id(6)).unwrap(),
        PostReturn::Post(_)
    ));
    writes.publish(by_id(6)).unwrap();
    assert_eq!(f.row(6).status.as_str(), "publish");
    writes.check_and_publish_future(by_id(999)).unwrap();
    assert_eq!(writes.add_ping(by_id(999), &s("x")).unwrap(), None);
    writes.delete_auto_drafts().unwrap();
    assert!(matches!(
        writes.delete(13, true).unwrap(),
        PostReturn::Post(_)
    ));

    let container = Rc::new(speedyspec_container::Container::new());
    register_post_write_handlers(&container, f.api.clone());
    for key in [
        <WpDeletePost as Command>::KEY,
        <WpTrashPost as Command>::KEY,
        <WpUntrashPost as Command>::KEY,
        <WpPublishPost as Command>::KEY,
        <CheckAndPublishFuturePost as Command>::KEY,
        <StickPost as Command>::KEY,
        <UnstickPost as Command>::KEY,
        <AddPing as Command>::KEY,
        <WpDeleteAutoDrafts as Command>::KEY,
        <ResetFrontPageSettingsForPost as Command>::KEY,
        <ResetPrivacyPolicyPageForPost as Command>::KEY,
    ] {
        assert!(container.has(key), "{key}");
    }
    let bus = speedyspec_cqrs::CommandBus::new(container.clone());
    let answer = bus
        .dispatch(WpDeletePost {
            post_id: 999,
            force: true,
        })
        .unwrap()
        .unwrap();
    assert_eq!(answer, PostReturn::Value(PhpValue::Null));
}
