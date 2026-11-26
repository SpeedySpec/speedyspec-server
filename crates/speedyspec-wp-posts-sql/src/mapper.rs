//! `PostMapper` (Data Mapper): a `wp_posts` row, `wp_insert_post()`'s column values, and the `posts` cache group's
//! entry, to the post entity and back (WordPress 7.2-RC2).

use speedyspec_data::{DbValue, Row};
use speedyspec_php_serialize::semantics::{get, to_php_int, to_php_string};
use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_posts_contracts::{
    CommentStatus, MimeType, PingStatus, Post, PostDate, PostDateTime, PostError, PostId,
    PostStatusName, PostTypeName, Slug,
};

/// Maps the post entity to and from its row, its insert values and its cache entry. It holds no state.
pub struct PostMapper;

/// A column's bytes as mysqli's text protocol gives them; `None` for a missing column or `NULL`.
fn bytes(value: Option<&DbValue>) -> Option<Vec<u8>> {
    value.and_then(DbValue::to_mysql_text)
}

/// Text when the bytes are UTF-8, bytes otherwise.
fn text_or_bytes(raw: &[u8]) -> DbValue {
    match String::from_utf8(raw.to_vec()) {
        Ok(text) => DbValue::Text(text),
        Err(e) => DbValue::Bytes(e.into_bytes()),
    }
}

/// Bytes as a name or datetime: lossily, as the contracts keep names.
fn lossy(raw: &[u8]) -> String {
    String::from_utf8_lossy(raw).into_owned()
}

/// One column's value, whatever its source: the row's or the cache entry's.
trait Columns {
    /// The column's bytes, or `None` when it is missing.
    fn text(&self, column: &str) -> Option<Vec<u8>>;
    /// An unsigned column (`post_author`, `post_parent`) as a `u64`, or `None` when it is missing.
    fn uint(&self, column: &str) -> Result<Option<u64>, PostError>;
    /// A signed column (`menu_order`, `comment_count`) as an `i64`, or `None` when it is missing.
    fn int(&self, column: &str) -> Result<Option<i64>, PostError>;
}

/// A `SELECT *` row: `NULL` text is `""`, as PHP's `(string) null` is; numbers parse strictly.
struct RowColumns<'a>(&'a Row);

impl RowColumns<'_> {
    /// A number column's text, parsed; a `NULL`, non-numeric or out-of-range value is corruption.
    fn number<T: std::str::FromStr>(&self, column: &str) -> Result<Option<T>, PostError> {
        let Some(value) = self.0.get(column) else {
            return Ok(None);
        };
        bytes(Some(value))
            .and_then(|b| String::from_utf8(b).ok())
            .and_then(|s| s.parse::<T>().ok())
            .map(Some)
            .ok_or_else(|| PostError::Db(format!("a post row has no usable {column}")))
    }
}

impl Columns for RowColumns<'_> {
    fn text(&self, column: &str) -> Option<Vec<u8>> {
        self.0
            .get(column)
            .map(|v| v.to_mysql_text().unwrap_or_default())
    }

    fn uint(&self, column: &str) -> Result<Option<u64>, PostError> {
        self.number(column)
    }

    fn int(&self, column: &str) -> Result<Option<i64>, PostError> {
        self.number(column)
    }
}

/// A cache entry's properties, read with PHP's `(string)` and `(int)` casts.
struct CacheColumns<'a>(&'a [(PhpKey, PhpValue)]);

impl Columns for CacheColumns<'_> {
    fn text(&self, column: &str) -> Option<Vec<u8>> {
        get(self.0, column).map(to_php_string)
    }

    /// `(int)`, with a negative value clamped to 0: the entity's unsigned fields cannot hold it.
    fn uint(&self, column: &str) -> Result<Option<u64>, PostError> {
        Ok(get(self.0, column).map(|v| u64::try_from(to_php_int(v)).unwrap_or(0)))
    }

    fn int(&self, column: &str) -> Result<Option<i64>, PostError> {
        Ok(get(self.0, column).map(to_php_int))
    }
}

/// The entity from its columns; a missing column takes [`Post::default()`]'s value.
fn entity(columns: &impl Columns, id: PostId) -> Result<Post, PostError> {
    let defaults = Post::default();
    let text = |column: &str, default: Vec<u8>| columns.text(column).unwrap_or(default);
    let datetime = |column: &str, default: PostDateTime| {
        columns
            .text(column)
            .map_or(default, |b| PostDateTime::new(lossy(&b)))
    };
    let name = |column: &str, default: &str| {
        columns
            .text(column)
            .map_or_else(|| default.to_owned(), |b| lossy(&b))
    };
    Ok(Post {
        id,
        author: columns.uint("post_author")?.unwrap_or(defaults.author),
        date: PostDate {
            local: datetime("post_date", defaults.date.local),
            gmt: datetime("post_date_gmt", defaults.date.gmt),
        },
        content: text("post_content", defaults.content),
        title: text("post_title", defaults.title),
        excerpt: text("post_excerpt", defaults.excerpt),
        status: PostStatusName::new(name("post_status", defaults.status.as_str())),
        comment_status: CommentStatus::new(name(
            "comment_status",
            defaults.comment_status.as_str(),
        )),
        ping_status: PingStatus::new(name("ping_status", defaults.ping_status.as_str())),
        password: text("post_password", defaults.password),
        name: Slug::new(text("post_name", defaults.name.as_bytes().to_vec())),
        to_ping: text("to_ping", defaults.to_ping),
        pinged: text("pinged", defaults.pinged),
        modified: PostDate {
            local: datetime("post_modified", defaults.modified.local),
            gmt: datetime("post_modified_gmt", defaults.modified.gmt),
        },
        content_filtered: text("post_content_filtered", defaults.content_filtered),
        parent: columns.uint("post_parent")?.map_or(defaults.parent, PostId),
        guid: text("guid", defaults.guid),
        menu_order: columns.int("menu_order")?.unwrap_or(defaults.menu_order),
        post_type: PostTypeName::new(name("post_type", defaults.post_type.as_str())),
        mime_type: MimeType::new(text(
            "post_mime_type",
            defaults.mime_type.as_bytes().to_vec(),
        )),
        comment_count: columns
            .int("comment_count")?
            .unwrap_or(defaults.comment_count),
    })
}

impl PostMapper {
    /// A `SELECT *` row. `ID` parses as `MetaMapper`'s ids do: a [`PostError::Db`] when it is missing, `NULL` or
    /// not a non-negative number. `ID`, `post_author` and `post_parent` are `u64`, and `menu_order` and
    /// `comment_count` `i64`; when present, a `NULL`, non-numeric, negative (for the unsigned three) or
    /// out-of-range number is a `Db` error, as the columns are `NOT NULL` and unsigned. A `NULL` text, name or
    /// date column is `""`, as PHP's `(string) null` is. A missing column takes [`Post::default()`]'s value,
    /// `WP_Post`'s property default.
    pub fn to_entity(row: &Row) -> Result<Post, PostError> {
        let id = bytes(row.get("ID"))
            .and_then(|b| String::from_utf8(b).ok())
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or_else(|| PostError::Db("a post row has no usable ID".to_owned()))?;
        entity(&RowColumns(row), PostId(id))
    }

    /// `wp_insert_post()`'s `$data`, in its `compact()` order (post.php, WordPress 7.2-RC2): `post_author`,
    /// `post_date`, `post_date_gmt`, `post_content`, `post_content_filtered`, `post_title`, `post_excerpt`,
    /// `post_status`, `post_type`, `comment_status`, `ping_status`, `post_password`, `post_name`, `to_ping`,
    /// `pinged`, `post_modified`, `post_modified_gmt`, `post_parent`, `menu_order`, `post_mime_type`, `guid`;
    /// then `ID` (the import ID) when the post's ID is not 0. Text is [`DbValue::Text`] when UTF-8 and
    /// [`DbValue::Bytes`] otherwise, as `MetaMapper` chooses; IDs are [`DbValue::UInt`], `menu_order`
    /// [`DbValue::Int`].
    pub fn insert_values(post: &Post) -> Vec<(String, DbValue)> {
        let text = |s: &str| DbValue::Text(s.to_owned());
        let mut values = vec![
            ("post_author", DbValue::UInt(post.author)),
            ("post_date", text(post.date.local.as_str())),
            ("post_date_gmt", text(post.date.gmt.as_str())),
            ("post_content", text_or_bytes(&post.content)),
            (
                "post_content_filtered",
                text_or_bytes(&post.content_filtered),
            ),
            ("post_title", text_or_bytes(&post.title)),
            ("post_excerpt", text_or_bytes(&post.excerpt)),
            ("post_status", text(post.status.as_str())),
            ("post_type", text(post.post_type.as_str())),
            ("comment_status", text(post.comment_status.as_str())),
            ("ping_status", text(post.ping_status.as_str())),
            ("post_password", text_or_bytes(&post.password)),
            ("post_name", text_or_bytes(post.name.as_bytes())),
            ("to_ping", text_or_bytes(&post.to_ping)),
            ("pinged", text_or_bytes(&post.pinged)),
            ("post_modified", text(post.modified.local.as_str())),
            ("post_modified_gmt", text(post.modified.gmt.as_str())),
            ("post_parent", DbValue::UInt(post.parent.0)),
            ("menu_order", DbValue::Int(post.menu_order)),
            ("post_mime_type", text_or_bytes(post.mime_type.as_bytes())),
            ("guid", text_or_bytes(&post.guid)),
        ];
        if post.id.0 != 0 {
            values.push(("ID", DbValue::UInt(post.id.0)));
        }
        values
            .into_iter()
            .map(|(column, value)| (column.to_owned(), value))
            .collect()
    }

    /// A `posts` cache group entry as `WP_Post::get_instance()` (class-wp-post.php 279-306) reads it.
    ///
    /// `None` is a miss, which WordPress answers by querying: an entry that is not a `stdClass` or `WP_Post`
    /// object, or whose `ID` is not set (missing or null). A subclass of `stdClass` also counts as a miss, an
    /// approximation: [`PhpValue`] carries a class name but no class hierarchy, so `instanceof` cannot be told.
    ///
    /// Otherwise the numbers go through [`to_php_int`] and the text through [`to_php_string`]. For an entry whose
    /// `filter` is not `raw`, `sanitize_post( …, 'raw' )` casts `ID`, `post_parent` and `menu_order` exactly so.
    /// The rest are approximations, because WordPress keeps the value as cached: a `raw` entry's non-int `ID`,
    /// `post_parent` or `menu_order`, and every entry's `post_author` and `comment_count`, which WordPress keeps
    /// as strings. A negative `ID`, `post_parent` or `post_author` is clamped to 0, since the entity's fields are
    /// unsigned; only this cache path clamps (a row reports it as [`PostError::Db`]). A missing column takes
    /// [`Post::default()`]'s value, as `WP_Post`'s property defaults do; any other property is ignored.
    pub fn from_cache(value: &PhpValue) -> Option<Post> {
        let PhpValue::Object { class, properties } = value else {
            return None;
        };
        if !class.eq_ignore_ascii_case("stdClass") && !class.eq_ignore_ascii_case("WP_Post") {
            return None;
        }
        let columns = CacheColumns(properties);
        get(properties, "ID").filter(|v| !matches!(v, PhpValue::Null))?;
        let id = columns.uint("ID").ok().flatten()?;
        Some(entity(&columns, PostId(id)).expect("cache columns always cast"))
    }

    /// The `stdClass` `get_instance()` caches after a miss: the 23 columns in table order, `ID`, `post_parent`
    /// and `menu_order` as ints, every other column a string (`post_author` and `comment_count` in decimal), then
    /// `filter` `raw`.
    ///
    /// An `ID` or `post_parent` above `i64::MAX` is `i64::MAX`. That is faithful: mysqli hands PHP the decimal
    /// string, and `sanitize_post()`'s `(int)` cast saturates it to `PHP_INT_MAX`.
    pub fn to_cache(post: &Post) -> PhpValue {
        let s = |b: &[u8]| PhpValue::Str(b.to_vec());
        let int = |i: u64| PhpValue::Int(i64::try_from(i).unwrap_or(i64::MAX));
        let properties = [
            ("ID", int(post.id.0)),
            ("post_author", s(post.author.to_string().as_bytes())),
            ("post_date", s(post.date.local.as_str().as_bytes())),
            ("post_date_gmt", s(post.date.gmt.as_str().as_bytes())),
            ("post_content", s(&post.content)),
            ("post_title", s(&post.title)),
            ("post_excerpt", s(&post.excerpt)),
            ("post_status", s(post.status.as_str().as_bytes())),
            ("comment_status", s(post.comment_status.as_str().as_bytes())),
            ("ping_status", s(post.ping_status.as_str().as_bytes())),
            ("post_password", s(&post.password)),
            ("post_name", s(post.name.as_bytes())),
            ("to_ping", s(&post.to_ping)),
            ("pinged", s(&post.pinged)),
            ("post_modified", s(post.modified.local.as_str().as_bytes())),
            (
                "post_modified_gmt",
                s(post.modified.gmt.as_str().as_bytes()),
            ),
            ("post_content_filtered", s(&post.content_filtered)),
            ("post_parent", int(post.parent.0)),
            ("guid", s(&post.guid)),
            ("menu_order", PhpValue::Int(post.menu_order)),
            ("post_type", s(post.post_type.as_str().as_bytes())),
            ("post_mime_type", s(post.mime_type.as_bytes())),
            (
                "comment_count",
                s(post.comment_count.to_string().as_bytes()),
            ),
            ("filter", s(b"raw")),
        ];
        PhpValue::Object {
            class: "stdClass".to_owned(),
            properties: properties
                .into_iter()
                .map(|(name, value)| (PhpKey::Str(name.as_bytes().to_vec()), value))
                .collect(),
        }
    }
}
