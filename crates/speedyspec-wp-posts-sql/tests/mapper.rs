//! `PostMapper` against WordPress 7.2-RC2: the rows `WP_Post::get_instance()` reads, mapped through `PostEntry` (the
//! read use cases' cache mapper) to the entry it caches, and the `posts` cache entries `PostEntry` accepts or treats
//! as a miss (`compat/posts-entity/cases.json`).

mod support;

use std::rc::Rc;

use speedyspec_data::{DbValue, Row};
use speedyspec_php_serialize::semantics::{get, to_php_string};
use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_posts::PostEntry;
use speedyspec_wp_posts_contracts::{
    CommentStatus, MimeType, PingStatus, Post, PostDate, PostDateTime, PostError, PostId,
    PostParent, PostRow, PostStatusName, PostTypeName, RowValue, Slug,
};
use speedyspec_wp_posts_sql::PostMapper;
use support::{cases, decode};

/// The 23 `wp_posts` columns, in table order.
const COLUMNS: [&str; 23] = [
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
];

fn row(cols: &[&str], values: Vec<DbValue>) -> Row {
    let cols: Rc<[String]> = cols.iter().map(|c| c.to_string()).collect();
    Row::new(cols, values)
}

/// A recorded row (a column => string array) as mysqli hands it over: text, or bytes when not UTF-8.
fn recorded_row(value: &PhpValue) -> Row {
    let PhpValue::Array(items) = value else {
        panic!("a row is an array");
    };
    let cols: Vec<String> = items
        .iter()
        .map(|(k, _)| match k {
            PhpKey::Str(s) => String::from_utf8(s.clone()).unwrap(),
            PhpKey::Int(i) => i.to_string(),
        })
        .collect();
    let values = items
        .iter()
        .map(|(_, v)| match String::from_utf8(to_php_string(v)) {
            Ok(text) => DbValue::Text(text),
            Err(e) => DbValue::Bytes(e.into_bytes()),
        })
        .collect();
    Row::new(cols.into(), values)
}

fn properties(value: &PhpValue) -> &[(PhpKey, PhpValue)] {
    match value {
        PhpValue::Array(items)
        | PhpValue::Object {
            properties: items, ..
        } => items,
        other => panic!("not an array or object: {other:?}"),
    }
}

#[test]
fn rows_map_to_the_cached_object() {
    let cases = cases();
    let rows = cases["rows"].as_array().unwrap();
    assert!(!rows.is_empty());
    for case in rows {
        let name = case["name"].as_str().unwrap();
        let entity = PostMapper::to_entity(&recorded_row(&decode(&case["row"])))
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(
            PostEntry::to_entry(&entity),
            decode(&case["cached"]),
            "{name}"
        );
    }
}

#[test]
fn cache_reads_match_wordpress() {
    let cases = cases();
    let reads = cases["cache_reads"].as_array().unwrap();
    assert!(!reads.is_empty());
    for case in reads {
        let name = case["name"].as_str().unwrap();
        let read = PostEntry::from_entry(&decode(&case["cached"]));
        assert_eq!(read.is_none(), case["queried"].as_bool().unwrap(), "{name}");
        let Some(entity) = read else { continue };
        let post = decode(&case["post"]);
        let ours = PostEntry::to_entry(&entity);
        for column in COLUMNS {
            assert_eq!(
                get(properties(&ours), column).map(to_php_string),
                get(properties(&post), column).map(to_php_string),
                "{name}: {column}"
            );
        }
    }
}

#[test]
fn entry_casts_numbers_and_defaults_missing_columns() {
    let s = |v: &str| PhpValue::Str(v.as_bytes().to_vec());
    let k = |v: &str| PhpKey::Str(v.as_bytes().to_vec());
    let entry = PhpValue::Object {
        class: "stdClass".into(),
        properties: vec![
            (k("ID"), s("12abc")),
            (k("post_parent"), s(" 4")),
            (k("menu_order"), s("-2")),
            (k("post_author"), s("7")),
            (k("comment_count"), PhpValue::Float(3.9)),
            (k("post_title"), PhpValue::Int(5)),
            (k("unknown"), s("ignored")),
        ],
    };
    let post = PostEntry::from_entry(&entry).unwrap();
    assert_eq!(post.id, PostId(12));
    assert_eq!(post.parent, PostParent::Int(4));
    assert_eq!(post.menu_order, -2);
    assert_eq!(post.author, 7);
    assert_eq!(post.comment_count, 3);
    assert_eq!(post.title, b"5".to_vec());
    assert_eq!(post.status, PostStatusName::new("publish"));
    assert_eq!(post.post_type, PostTypeName::new("post"));
    assert_eq!(post.date, PostDate::default());

    let null_id = PhpValue::Object {
        class: "WP_Post".into(),
        properties: vec![(k("ID"), PhpValue::Null)],
    };
    assert!(PostEntry::from_entry(&null_id).is_none());
    let other_class = PhpValue::Object {
        class: "ArrayObject".into(),
        properties: vec![(k("ID"), PhpValue::Int(3))],
    };
    assert!(PostEntry::from_entry(&other_class).is_none());
}

fn sample(id: u64) -> Post {
    Post {
        id: PostId(id),
        author: 5,
        date: PostDate {
            local: PostDateTime::new("2024-03-01 06:00:00"),
            gmt: PostDateTime::new("2024-03-01 12:00:00"),
        },
        content: b"body".to_vec(),
        title: b"Title".to_vec(),
        excerpt: b"ex".to_vec(),
        status: PostStatusName::new("draft"),
        comment_status: CommentStatus::new("closed"),
        ping_status: PingStatus::new("open"),
        password: b"pw".to_vec(),
        name: Slug::from("title"),
        to_ping: Vec::new(),
        pinged: Vec::new(),
        modified: PostDate::default(),
        content_filtered: Vec::new(),
        parent: PostParent::Int(2),
        guid: b"http://example.org/?p=1".to_vec(),
        menu_order: -1,
        post_type: PostTypeName::new("page"),
        mime_type: MimeType::from("text/plain"),
        comment_count: 9,
    }
}

#[test]
fn row_values_keep_the_row_order_and_import_id() {
    let text = |v: &str| DbValue::Text(v.into());
    let expected = vec![
        ("post_author", DbValue::Int(5)),
        ("post_date", text("2024-03-01 06:00:00")),
        ("post_date_gmt", text("2024-03-01 12:00:00")),
        ("post_content", text("body")),
        ("post_content_filtered", text("")),
        ("post_title", text("Title")),
        ("post_excerpt", text("ex")),
        ("post_status", text("draft")),
        ("post_type", text("page")),
        ("comment_status", text("closed")),
        ("ping_status", text("open")),
        ("post_password", text("pw")),
        ("post_name", text("title")),
        ("to_ping", text("")),
        ("pinged", text("")),
        ("post_modified", text("0000-00-00 00:00:00")),
        ("post_modified_gmt", text("0000-00-00 00:00:00")),
        ("post_parent", DbValue::Int(2)),
        ("menu_order", DbValue::Int(-1)),
        ("post_mime_type", text("text/plain")),
        ("guid", text("http://example.org/?p=1")),
    ];
    let expected: Vec<(String, DbValue)> = expected
        .into_iter()
        .map(|(c, v)| (c.to_string(), v))
        .collect();
    let values = |row: &PostRow| PostMapper::row_values(row).unwrap();
    let new = values(&PostRow::from_post(&sample(14)));
    assert_eq!(new.len(), 21);
    assert_eq!(new, expected);

    let imported = values(&PostRow::from_post(&sample(0)).with("ID", RowValue::Int(14)));
    assert_eq!(imported.len(), 22);
    assert_eq!(imported[..21], expected[..]);
    assert_eq!(imported[21], ("ID".to_string(), DbValue::Int(14)));

    let negative = values(&PostRow::from_post(&sample(0)).with("ID", RowValue::Int(-5)));
    assert_eq!(negative[21], ("ID".to_string(), DbValue::Int(-5)));

    let orphan = values(&PostRow::from_post(&Post {
        parent: PostParent::Null,
        ..sample(0)
    }));
    assert_eq!(orphan[17], ("post_parent".to_string(), DbValue::Null));

    // A dropped column is left out, an added key kept, and a value that does not fit its placeholder refused.
    let mut dropped = PostRow::from_post(&sample(0)).with("bogus", RowValue::Str(b"1".to_vec()));
    dropped.remove("post_content");
    let filtered = values(&dropped);
    assert_eq!(filtered.len(), 21);
    assert!(filtered.iter().all(|(c, _)| c != "post_content"));
    assert_eq!(
        filtered[20],
        ("bogus".to_string(), DbValue::Text("1".into()))
    );
    assert!(matches!(
        PostMapper::row_values(&PostRow::new().with("menu_order", RowValue::Str(b"4".to_vec()))),
        Err(PostError::Db(_))
    ));
}

#[test]
fn missing_or_bad_id_is_db_error() {
    let missing = row(
        &["post_author", "post_title"],
        vec![DbValue::Int(1), DbValue::Text("t".into())],
    );
    assert!(matches!(
        PostMapper::to_entity(&missing),
        Err(PostError::Db(_))
    ));
    for bad in [
        DbValue::Text("x".into()),
        DbValue::Null,
        DbValue::Text("-1".into()),
    ] {
        let r = row(&["ID", "post_title"], vec![bad, DbValue::Text("t".into())]);
        assert!(matches!(PostMapper::to_entity(&r), Err(PostError::Db(_))));
    }
    let numeric = row(
        &["ID", "post_parent"],
        vec![DbValue::UInt(8), DbValue::Bytes(b"3".to_vec())],
    );
    let post = PostMapper::to_entity(&numeric).unwrap();
    assert_eq!(post.id, PostId(8));
    assert_eq!(post.parent, PostParent::Int(3));
}

#[test]
fn negative_or_null_numbers_in_a_row_are_db_errors() {
    for column in ["post_author", "post_parent"] {
        for bad in [DbValue::Text("-1".into()), DbValue::Int(-1), DbValue::Null] {
            let r = row(&["ID", column], vec![DbValue::UInt(1), bad.clone()]);
            assert!(
                matches!(PostMapper::to_entity(&r), Err(PostError::Db(_))),
                "{column} {bad:?}"
            );
        }
    }
    let r = row(&["ID", "menu_order"], vec![DbValue::UInt(1), DbValue::Null]);
    assert!(matches!(PostMapper::to_entity(&r), Err(PostError::Db(_))));
}

#[test]
fn unsigned_columns_above_i64_max_parse() {
    let big = u64::MAX;
    let r = row(
        &["ID", "post_author", "post_parent"],
        vec![
            DbValue::Text(big.to_string()),
            DbValue::UInt(big),
            DbValue::Text((i64::MAX as u64 + 1).to_string()),
        ],
    );
    let post = PostMapper::to_entity(&r).unwrap();
    assert_eq!(post.id, PostId(big));
    assert_eq!(post.author, big);
    // `(int)` reads a parent past `PHP_INT_MAX` as `PHP_INT_MAX`.
    assert_eq!(post.parent, PostParent::Int(i64::MAX));
    // `(int)` saturates the decimal string, as sanitize_post() does; the author stays a string.
    let cached = PostEntry::to_entry(&post);
    let PhpValue::Object { properties, .. } = &cached else {
        panic!("an object");
    };
    assert_eq!(get(properties, "ID"), Some(&PhpValue::Int(i64::MAX)));
    assert_eq!(
        get(properties, "post_parent"),
        Some(&PhpValue::Int(i64::MAX))
    );
    assert_eq!(
        get(properties, "post_author"),
        Some(&PhpValue::Str(big.to_string().into_bytes()))
    );
}

#[test]
fn null_text_names_and_dates_in_a_row_are_empty_and_missing_ones_default() {
    let null_columns = [
        "ID",
        "post_title",
        "post_status",
        "comment_status",
        "ping_status",
        "post_type",
        "post_date",
        "post_modified_gmt",
    ];
    let mut values = vec![DbValue::UInt(1)];
    values.resize(null_columns.len(), DbValue::Null);
    let post = PostMapper::to_entity(&row(&null_columns, values)).unwrap();
    assert_eq!(post.title, Vec::<u8>::new());
    assert_eq!(post.status, PostStatusName::new(""));
    assert_eq!(post.comment_status, CommentStatus::new(""));
    assert_eq!(post.ping_status, PingStatus::new(""));
    assert_eq!(post.post_type, PostTypeName::new(""));
    assert_eq!(post.date.local, PostDateTime::new(""));
    assert_eq!(post.modified.gmt, PostDateTime::new(""));
    // Missing columns take WP_Post's property defaults.
    assert_eq!(post.date.gmt, PostDateTime::ZERO);
    assert_eq!(post.modified.local, PostDateTime::ZERO);

    let missing = PostMapper::to_entity(&row(&["ID"], vec![DbValue::UInt(1)])).unwrap();
    assert_eq!(
        missing,
        Post {
            id: PostId(1),
            ..Post::default()
        }
    );
}

#[test]
fn cache_clamps_negative_unsigned_columns() {
    let k = |v: &str| PhpKey::Str(v.as_bytes().to_vec());
    let entry = PhpValue::Object {
        class: "stdClass".into(),
        properties: vec![
            (k("ID"), PhpValue::Int(3)),
            (k("post_author"), PhpValue::Str(b"-1".to_vec())),
            (k("post_parent"), PhpValue::Int(-2)),
        ],
    };
    let post = PostEntry::from_entry(&entry).unwrap();
    assert_eq!(post.author, 0);
    assert_eq!(post.parent, PostParent::Int(0));
}

#[test]
fn non_utf8_text_is_bytes() {
    let mut post = sample(0);
    post.title = vec![0xFF, 0xFE];
    post.name = Slug::new(vec![0xC3]);
    post.content = "Café".as_bytes().to_vec();
    let values = PostMapper::row_values(&PostRow::from_post(&post)).unwrap();
    let value = |column: &str| {
        values
            .iter()
            .find(|(c, _)| c == column)
            .map(|(_, v)| v.clone())
            .unwrap()
    };
    assert_eq!(value("post_title"), DbValue::Bytes(vec![0xFF, 0xFE]));
    assert_eq!(value("post_name"), DbValue::Bytes(vec![0xC3]));
    assert_eq!(value("post_content"), DbValue::Text("Café".into()));

    let r = row(
        &["ID", "post_title", "post_content"],
        vec![DbValue::Int(1), DbValue::Bytes(vec![0xFF]), DbValue::Null],
    );
    let entity = PostMapper::to_entity(&r).unwrap();
    assert_eq!(entity.title, vec![0xFF]);
    assert_eq!(entity.content, Vec::<u8>::new());
}
