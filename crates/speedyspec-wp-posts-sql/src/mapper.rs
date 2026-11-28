//! `PostMapper` (Data Mapper): a `wp_posts` row and `wp_insert_post()`'s column values, to the post entity and back
//! (WordPress 7.2-RC2). The `posts` cache group's entry is the read use cases' (`speedyspec_wp_posts::PostEntry`).

use speedyspec_data::{DbValue, Row};
use speedyspec_php_serialize::PhpValue;
use speedyspec_php_serialize::semantics::to_php_int;
use speedyspec_wp_db_contracts::WpValue;
use speedyspec_wp_posts_contracts::{
    ColumnFormat, CommentStatus, MimeType, PingStatus, Post, PostDate, PostDateTime, PostError,
    PostId, PostParent, PostRow, PostStatusName, PostTypeName, RowValue, Slug, column_format,
};

/// Maps the post entity to and from its row and its insert values. It holds no state.
pub struct PostMapper;

/// A column's bytes as mysqli's text protocol gives them; `None` for a missing column or `NULL`.
fn bytes(value: Option<&DbValue>) -> Option<Vec<u8>> {
    value.and_then(DbValue::to_mysql_text)
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

/// `mysqli_real_escape_string()`: a backslash before NUL, `\n`, `\r`, `\\`, `'`, `"` and `\x1a`.
fn escape(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    for &b in bytes {
        match b {
            0 => out.extend_from_slice(b"\\0"),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            0x1a => out.extend_from_slice(b"\\Z"),
            b'\\' | b'\'' | b'"' => {
                out.push(b'\\');
                out.push(b);
            }
            b => out.push(b),
        }
    }
    out
}

/// A string under `%d` as `prepare()` prints it: escaped by `mysqli_real_escape_string()` first, then read by
/// `vsprintf( '%d' )`, which converts with PHP's integer rules (`zval_get_long()`): the whole numeric prefix after
/// leading whitespace, decimals and exponent included. So `'4x'` is `4`, `'1e3'` is `1000`, `'1.5e2'` is `150`,
/// `'0x1A'` is `0`, an infinite prefix (`'1e400'`) is `0`, an out-of-range one saturates, and `"\n5"` is `0` because the
/// newline is escaped first. Printed here, so every engine stores WordPress's integer (SQLite would keep a bound
/// `'4x'` as text), and the memory repository and this one agree.
fn escaped_int(bytes: &[u8]) -> i64 {
    to_php_int(&PhpValue::Str(escape(bytes)))
}

/// A float or bool under `%d` as `prepare()` prints it (`sprintf( '%d' )`: a float truncated, a bool `1` or `0`),
/// so the engine receives WordPress's integer whatever its own float formatting is; under `%s`, and for any other
/// value, the PHP value as it is, for `wpdb` to format (and to leave unchecked by `strip_invalid_text()`, which reads
/// only strings).
fn under_placeholder(column: &[u8], value: &WpValue) -> WpValue {
    if column_format(column) != ColumnFormat::Int {
        return value.clone();
    }
    match value {
        // `zend_dval_to_lval()`: truncation, wrapping modulo 2^64 outside the int range.
        WpValue::Float(f) if f.is_finite() => {
            WpValue::Int((f.trunc() % 18_446_744_073_709_551_616.0) as i128 as i64)
        }
        WpValue::Float(_) => WpValue::Int(0),
        WpValue::Bool(b) => WpValue::Int(i64::from(*b)),
        other => other.clone(),
    }
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

    /// The filtered `$data` ([`PostRow`]) as `$wpdb->insert` and `$wpdb->update` take it: its columns in order, each
    /// value the PHP value it is, so `wpdb` formats it as [`RowValue`] documents (an int, a string, `null`, or a float,
    /// bool, array or object as the hooks carry it). A string is text when UTF-8 and bytes otherwise; a key that is
    /// not UTF-8 is read lossily, since `wpdb`'s port names columns by text (a known limitation: such a key names no
    /// column either way, so the statement fails as WordPress's would).
    pub fn row_values(row: &PostRow) -> Vec<(String, WpValue)> {
        row.columns()
            .iter()
            .map(|(column, value)| {
                // Not an exhaustive match: the contract's value type may grow.
                let wp = if let RowValue::Int(n) = value {
                    WpValue::Int(*n)
                } else if let RowValue::Str(bytes) = value
                    && column_format(column) == ColumnFormat::Int
                {
                    WpValue::Int(escaped_int(bytes))
                } else if let RowValue::Str(bytes) = value {
                    match String::from_utf8(bytes.clone()) {
                        Ok(text) => WpValue::Str(text),
                        Err(e) => WpValue::Bytes(e.into_bytes()),
                    }
                } else if let RowValue::Php(php) = value {
                    under_placeholder(column, php)
                } else {
                    WpValue::Null
                };
                (String::from_utf8_lossy(column).into_owned(), wp)
            })
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
