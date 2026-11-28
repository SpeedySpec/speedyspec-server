//! `PostMapper` (Data Mapper): a `wp_posts` row and `wp_insert_post()`'s column values, to the post entity and back
//! (WordPress 7.2-RC2). The `posts` cache group's entry is the read use cases' (`speedyspec_wp_posts::PostEntry`).

use speedyspec_data::{DbValue, Row};
use speedyspec_wp_posts_contracts::{
    CommentStatus, MimeType, PingStatus, Post, PostDate, PostDateTime, PostError, PostId,
    PostParent, PostStatusName, PostTypeName, Slug,
};

/// Maps the post entity to and from its row and its insert values. It holds no state.
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

/// One column's value.
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
        parent: columns
            .uint("post_parent")?
            .map_or(defaults.parent, |id| PostParent::from(PostId(id))),
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
    /// then `ID` when there is an `import_id`, sent as it is (`0` and negative values included). `post.id` is not
    /// read. Text is [`DbValue::Text`] when UTF-8 and [`DbValue::Bytes`] otherwise, as `MetaMapper` chooses;
    /// `post_author` is [`DbValue::UInt`], `post_parent`, `menu_order` and `ID` [`DbValue::Int`], and a
    /// [`PostParent::Null`] parent [`DbValue::Null`], which `wpdb` writes as `NULL`.
    pub fn insert_values(post: &Post, import_id: Option<i64>) -> Vec<(String, DbValue)> {
        let mut values = Self::update_values(post);
        if let Some(id) = import_id {
            values.push(("ID".to_owned(), DbValue::Int(id)));
        }
        values
    }

    /// The same 21 columns as [`PostMapper::insert_values`], in its order, without `ID`: `wp_insert_post()`'s
    /// `$data` for `$wpdb->update`.
    pub fn update_values(post: &Post) -> Vec<(String, DbValue)> {
        let text = |s: &str| DbValue::Text(s.to_owned());
        let values = vec![
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
            ("post_parent", Self::parent_value(post.parent)),
            ("menu_order", DbValue::Int(post.menu_order)),
            ("post_mime_type", text_or_bytes(post.mime_type.as_bytes())),
            ("guid", text_or_bytes(&post.guid)),
        ];
        values
            .into_iter()
            .map(|(column, value)| (column.to_owned(), value))
            .collect()
    }

    /// A parent as `$data['post_parent']` holds it: the integer, or `null`.
    pub fn parent_value(parent: PostParent) -> DbValue {
        match parent {
            PostParent::Int(n) => DbValue::Int(n),
            PostParent::Null => DbValue::Null,
        }
    }
}
