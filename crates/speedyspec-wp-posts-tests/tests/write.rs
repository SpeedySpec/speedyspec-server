//! The post write use cases (`PostWritesApi`) over `MemoryPostRepository`, `WpHookApi` over the memory store, the
//! Rust object cache, phase 3's `MetadataApi` over memory meta, the registries `create_initial_post_types()` fills,
//! and port fakes that answer as WordPress 7.2-RC2 does on the recorded site (compat/posts-write/setup.php).
//!
//! The replay runs the recorded `insert-*`, `update-*`, `transition-*` and `set-type` steps that are pure in Rust
//! (no kses, no terms read back, no attachment files) and compares what each returned, every fixture callback's run
//! with its arguments, `wp_posts` and `wp_postmeta` whole, and the cron events. What WordPress's own revision code
//! and `post_updated` callbacks wrote is left out (they stay PHP until their phases), and so is `comment_count`
//! (see `without_comment_count()`). `transition-guid-reset` goes through `wp_publish_post()`, which phase 5b ports.

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
    PostDateTime, PostError, PostId, PostObject, PostParent, PostQuery, PostRepository,
    PostStatusName, PostType, PostTypeName, PostTypeSupports, PostTypes, Rewrite, RewriteContext,
    Slug, SlugTruncation, Taxonomy, Theme, Timezone, TitleSanitizer, Translator, Uploads, Users,
    WriteResult,
};
use speedyspec_wp_posts_memory::MemoryPostRepository;

// ---- the clock: 2026-11-27 12:00:00 GMT, `gmt_offset` 2 ----

const NOW: i64 = 1_795_780_800;
const OFFSET: i64 = 2 * 3600;

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

struct FixedClock;
impl Clock for FixedClock {
    fn now_local(&self) -> String {
        format(NOW + OFFSET)
    }
    fn now_gmt(&self) -> String {
        format(NOW)
    }
    fn date_from_gmt(&self, gmt: &[u8]) -> Vec<u8> {
        parse(gmt).map_or(gmt.to_vec(), |t| format(t + OFFSET).into_bytes())
    }
    fn gmt_from_date(&self, local: &[u8]) -> Vec<u8> {
        parse(local).map_or(local.to_vec(), |t| format(t - OFFSET).into_bytes())
    }
    fn timestamp(&self, datetime: &[u8]) -> Option<i64> {
        parse(datetime)
    }
}

impl Microtime for FixedClock {
    fn microtime(&self) -> String {
        format!("0.00000000 {NOW}")
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
struct DefaultComments(Rc<Types>);
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
        } else if self.0.supports(post_type.as_str(), supports) {
            "open"
        } else {
            "closed"
        }))
    }
    fn trash_post_comments(&self, _: PostId) -> Result<(), PostError> {
        Ok(())
    }
    fn untrash_post_comments(&self, _: PostId) -> Result<(), PostError> {
        Ok(())
    }
    fn delete_post_comments(&self, _: PostId) -> Result<(), PostError> {
        Ok(())
    }
    fn defer_counting(&self, _: bool) -> Result<(), PostError> {
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
    fn translate(&self, text: &str, _: Option<&str>) -> String {
        text.to_owned()
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
}

/// The site's options.
struct SiteOptions;
impl Options for SiteOptions {
    fn get_option(
        &self,
        name: &str,
        default: Option<HookValue>,
    ) -> Result<HookValue, OptionsError> {
        Ok(match name {
            "default_category" | "default_pingback_flag" => HookValue::Str("1".into()),
            "sticky_posts" => HookValue::Array(Vec::new()),
            _ => default.unwrap_or(HookValue::Bool(false)),
        })
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
    fn update_option(&self, _: &str, _: HookValue, _: HookValue) -> Result<bool, OptionsError> {
        Ok(false)
    }
    fn delete_option(&self, _: &str) -> Result<bool, OptionsError> {
        Ok(false)
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
        insert(post: &Post, import_id: Option<i64>) -> Result<Option<PostId>, PostError>;
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
        update(post: &Post) -> Result<Option<u64>, PostError>;
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
        repo.insert(post, Some(post.id.0 as i64)).unwrap();
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
    );
    let global: Rc<Global> = Rc::default();
    let terms: Rc<Terms> = Rc::default();
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
        options: Rc::new(SiteOptions),
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
        comments: Rc::new(DefaultComments(types.clone())),
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
    });
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
                .register(name.clone(), args, &ports());
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
        other => Var::Value(plain_value(other)),
    }
}

fn plain_value(value: &serde_json::Value) -> PhpValue {
    match value {
        serde_json::Value::Null => PhpValue::Null,
        serde_json::Value::Bool(b) => PhpValue::Bool(*b),
        serde_json::Value::Number(n) => n
            .as_i64()
            .map_or_else(|| PhpValue::Float(n.as_f64().unwrap()), PhpValue::Int),
        serde_json::Value::String(t) => s(t),
        serde_json::Value::Array(items) => PhpValue::Array(
            items
                .iter()
                .enumerate()
                .map(|(i, v)| (PhpKey::Int(i as i64), plain_value(v)))
                .collect(),
        ),
        serde_json::Value::Object(map) => PhpValue::Array(
            map.iter()
                .map(|(n, v)| (support::php_key(n), plain_value(v)))
                .collect(),
        ),
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

/// `comment_count` dropped at any depth: the seed goes through the repository's insert, which (as `wp_insert_post()`'s
/// `$wpdb->insert`) never writes it, so seeded posts read 0 where the recorded site has comments. No writer here sets
/// it.
fn without_comment_count(value: PhpValue) -> PhpValue {
    match value {
        PhpValue::Array(items) => PhpValue::Array(
            items
                .into_iter()
                .filter(|(k, _)| *k != PhpKey::Str(b"comment_count".to_vec()))
                .map(|(k, v)| (k, without_comment_count(v)))
                .collect(),
        ),
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
                // Term reads are the taxonomy code's (WordPress's until it is ported): not replayed.
                "wp_get_post_categories" | "wp_get_post_terms" | "wp_get_object_terms" => continue,
                other => panic!("{at}: {other} is not replayed"),
            };
            if let Some(returned) = returned
                && !result["returned"].is_object()
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
                .collect();
            let want: Vec<_> = support::recorded_invocations(result)
                .into_iter()
                .filter(|(_, a)| !from_revision(a))
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
                let (got, want) = (
                    without_comment_count(got.clone()),
                    without_comment_count(want.clone()),
                );
                if let Some(path) = first_difference(&got, &want, String::new()) {
                    panic!("{at}: {label}'s arguments differ at {path}");
                }
            }
            let snapshot = result.get("snapshot");
            if let Some(rows) = snapshot.and_then(|s| snapshot_table(s, "wp_posts")) {
                // The revisions WordPress's revision code wrote are left out (see from_revision()), and so is
                // comment_count, which no writer here sets (the comment code keeps it).
                let comparable = |row: Vec<(String, String)>| {
                    sorted(
                        row.into_iter()
                            .filter(|(c, _)| c != "comment_count")
                            .collect(),
                    )
                };
                let want: Vec<_> = rows
                    .into_iter()
                    .filter(|r| {
                        col(r, "post_type") != "revision"
                            || col(r, "ID").parse::<u64>().unwrap() < 26
                    })
                    .map(comparable)
                    .collect();
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
