//! `PostEntry` (Data Mapper): the `posts` cache group's entry and `WP_Post`'s properties, to the post entity and
//! back. The entry stays WordPress's `stdClass` (Identity Map), so an entry another PHP process wrote is read as is.
//!
//! It lives beside the use cases because the cache entry is theirs, not the database's: the SQL adapter's
//! `PostMapper` maps rows. Its cache half (`from_cache`/`to_cache`) is the same mapping and can delegate here.

use speedyspec_php_serialize::semantics::{get, isset, set, to_php_int, to_php_string};
use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_posts_contracts::{
    CommentStatus, MimeType, PingStatus, Post, PostDate, PostDateTime, PostId, PostStatusName,
    PostTypeName, Slug,
};

/// Maps the post entity to and from the `posts` cache entry and `WP_Post`'s properties. It holds no state.
pub struct PostEntry;

/// `WP_Post`'s declared properties, in class order; `filter` is declared last with no default (null).
const DECLARED: [&str; 24] = [
    "ID",
    "post_author",
    "post_date",
    "post_date_gmt",
    "post_content",
    "post_title",
    "post_excerpt",
    "post_status",
    "comment_status",
    "ping_status",
    "post_password",
    "post_name",
    "to_ping",
    "pinged",
    "post_modified",
    "post_modified_gmt",
    "post_content_filtered",
    "post_parent",
    "guid",
    "menu_order",
    "post_type",
    "post_mime_type",
    "comment_count",
    "filter",
];

fn key(name: &str) -> PhpKey {
    PhpKey::Str(name.as_bytes().to_vec())
}

impl PostEntry {
    /// Whether `name` is one of `WP_Post`'s declared properties (the 23 columns and `filter`).
    pub fn is_declared(name: &[u8]) -> bool {
        DECLARED.iter().any(|d| d.as_bytes() == name)
    }

    /// The entry's properties when `WP_Post::get_instance()` (class-wp-post.php 287-290) can use it: a `stdClass`
    /// or `WP_Post` object whose `ID` is set (present and not null). `None` is a miss, which WordPress answers by
    /// querying and replacing the entry. A subclass of `stdClass` counts as a miss, an approximation: [`PhpValue`]
    /// carries a class name but no class hierarchy, so `instanceof` cannot be told.
    pub fn usable(value: &PhpValue) -> Option<&[(PhpKey, PhpValue)]> {
        let PhpValue::Object { class, properties } = value else {
            return None;
        };
        if !class.eq_ignore_ascii_case("stdClass") && !class.eq_ignore_ascii_case("WP_Post") {
            return None;
        }
        isset(properties, "ID")?;
        Some(properties)
    }

    /// The entity from an object's properties, read with PHP's `(string)` and `(int)` casts. A missing property
    /// takes [`Post::default()`]'s value, as `WP_Post`'s property defaults do; any other property is ignored. A
    /// negative `ID`, `post_parent` or `post_author` is 0, since the entity's fields are unsigned.
    pub fn from_properties(properties: &[(PhpKey, PhpValue)]) -> Post {
        let defaults = Post::default();
        let text =
            |name: &str, default: Vec<u8>| get(properties, name).map_or(default, to_php_string);
        let lossy = |b: Vec<u8>| String::from_utf8_lossy(&b).into_owned();
        let datetime = |name: &str, default: PostDateTime| {
            get(properties, name).map_or(default, |v| PostDateTime::new(lossy(to_php_string(v))))
        };
        let word = |name: &str, default: &str| {
            get(properties, name).map_or_else(|| default.to_owned(), |v| lossy(to_php_string(v)))
        };
        let uint = |name: &str, default: u64| {
            get(properties, name).map_or(default, |v| u64::try_from(to_php_int(v)).unwrap_or(0))
        };
        let int = |name: &str, default: i64| get(properties, name).map_or(default, to_php_int);
        Post {
            id: PostId(uint("ID", 0)),
            author: uint("post_author", defaults.author),
            date: PostDate {
                local: datetime("post_date", defaults.date.local.clone()),
                gmt: datetime("post_date_gmt", defaults.date.gmt.clone()),
            },
            content: text("post_content", defaults.content.clone()),
            title: text("post_title", defaults.title.clone()),
            excerpt: text("post_excerpt", defaults.excerpt.clone()),
            status: PostStatusName::new(word("post_status", defaults.status.as_str())),
            comment_status: CommentStatus::new(word(
                "comment_status",
                defaults.comment_status.as_str(),
            )),
            ping_status: PingStatus::new(word("ping_status", defaults.ping_status.as_str())),
            password: text("post_password", defaults.password.clone()),
            name: Slug::new(text("post_name", defaults.name.as_bytes().to_vec())),
            to_ping: text("to_ping", defaults.to_ping.clone()),
            pinged: text("pinged", defaults.pinged.clone()),
            modified: PostDate {
                local: datetime("post_modified", defaults.modified.local.clone()),
                gmt: datetime("post_modified_gmt", defaults.modified.gmt.clone()),
            },
            content_filtered: text("post_content_filtered", defaults.content_filtered.clone()),
            parent: PostId(uint("post_parent", defaults.parent.0)),
            guid: text("guid", defaults.guid.clone()),
            menu_order: int("menu_order", defaults.menu_order),
            post_type: PostTypeName::new(word("post_type", defaults.post_type.as_str())),
            mime_type: MimeType::new(text(
                "post_mime_type",
                defaults.mime_type.as_bytes().to_vec(),
            )),
            comment_count: int("comment_count", defaults.comment_count),
        }
    }

    /// The `stdClass` `get_instance()` caches after a miss (`sanitize_post( $row, 'raw' )` of the row): the 23
    /// columns in table order, `ID`, `post_parent` and `menu_order` as ints, every other column a string
    /// (`post_author` and `comment_count` in decimal), then `filter` `raw`. An id above `i64::MAX` is
    /// `PHP_INT_MAX`, as `(int)` saturates it.
    pub fn to_entry(post: &Post) -> PhpValue {
        PhpValue::Object {
            class: "stdClass".to_owned(),
            properties: Self::raw_properties(post),
        }
    }

    /// [`PostEntry::to_entry`]'s properties.
    pub fn raw_properties(post: &Post) -> Vec<(PhpKey, PhpValue)> {
        let s = |b: &[u8]| PhpValue::Str(b.to_vec());
        let int = |i: u64| PhpValue::Int(i64::try_from(i).unwrap_or(i64::MAX));
        [
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
        ]
        .into_iter()
        .map(|(name, value)| (key(name), value))
        .collect()
    }

    /// `new WP_Post( $object )`'s properties: the declared ones in class order with their defaults (`ID` null,
    /// `post_parent` and `menu_order` 0, the dates zero, the strings `''`, `post_author` and `comment_count` `'0'`, `post_status`
    /// `publish`, `comment_status` and `ping_status` `open`, `post_type` `post`, `filter` null), each overwritten by
    /// the object's value, then the object's other properties in their order.
    pub fn wp_post_properties(object: &[(PhpKey, PhpValue)]) -> Vec<(PhpKey, PhpValue)> {
        let mut properties = Self::raw_properties(&Post::default());
        set(&mut properties, "ID", PhpValue::Null);
        set(&mut properties, "filter", PhpValue::Null);
        for (k, v) in object {
            match properties.iter_mut().find(|(pk, _)| pk == k) {
                Some(slot) => slot.1 = v.clone(),
                None => properties.push((k.clone(), v.clone())),
            }
        }
        properties
    }

    /// A `WP_Post` object with these properties.
    pub fn wp_post(properties: Vec<(PhpKey, PhpValue)>) -> PhpValue {
        PhpValue::Object {
            class: "WP_Post".to_owned(),
            properties,
        }
    }
}
