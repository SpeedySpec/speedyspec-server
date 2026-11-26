//! The checks every post repository passes, so the memory and SQL repositories answer alike.

use crate::{
    CommentStatus, MimeType, PingStatus, Post, PostDate, PostDateTime, PostId, PostRepository,
    PostStatusName, PostTypeName, Slug,
};

fn ensure(check: &str, ok: bool, detail: impl FnOnce() -> String) {
    assert!(ok, "post repository check `{check}` failed: {}", detail());
}

fn slug(s: &str) -> Slug {
    Slug::from(s)
}

fn post_type(s: &str) -> PostTypeName {
    PostTypeName::new(s)
}

/// A post of `post_type` named `name` under `parent`, with an auto id.
fn named(name: &str, kind: &str, parent: u64) -> Post {
    Post {
        name: slug(name),
        post_type: post_type(kind),
        parent: PostId(parent),
        ..Post::default()
    }
}

/// Runs every check against `repo`, which must start with an empty table. Panics naming the failing check.
pub fn post_repository_suite(repo: &dyn PostRepository) {
    let insert = |post: &Post| {
        repo.insert(post)
            .expect("insert")
            .expect("the insert returns an id")
    };
    let find = |id: PostId| repo.find(id).expect("find");

    // insert_returns_increasing_ids
    let a = insert(&named("first", "post", 0));
    let b = insert(&named("second", "post", 0));
    let c = insert(&named("third", "page", 0));
    ensure(
        "insert_returns_increasing_ids",
        PostId(0) < a && a < b && b < c,
        || format!("{a:?} {b:?} {c:?}"),
    );

    // insert_with_id_is_honoured: the auto-increment continues past an explicit ID, as MySQL's does.
    let explicit = insert(&Post {
        id: PostId(100),
        ..named("imported", "post", 0)
    });
    let next = insert(&named("after-import", "post", 0));
    ensure(
        "insert_with_id_is_honoured",
        explicit == PostId(100) && next == PostId(101),
        || format!("{explicit:?} {next:?}"),
    );

    // insert_taken_id_is_none
    let taken = repo
        .insert(&Post {
            id: PostId(100),
            ..named("duplicate", "post", 0)
        })
        .expect("insert");
    let kept = find(PostId(100)).map(|p| p.name);
    ensure(
        "insert_taken_id_is_none",
        taken.is_none() && kept == Some(slug("imported")),
        || format!("{taken:?} {kept:?}"),
    );

    // find_returns_what_insert_wrote: every column, a zero date among them.
    let full = Post {
        id: PostId(0),
        author: 7,
        date: PostDate {
            local: PostDateTime::new("2024-01-05 10:20:30"),
            gmt: PostDateTime::ZERO,
        },
        content: b"<p>Hello, \xc3\xa9t\xc3\xa9</p>\x00tail".to_vec(),
        title: b"A title".to_vec(),
        excerpt: b"An excerpt".to_vec(),
        status: PostStatusName::new("draft"),
        comment_status: CommentStatus::new("closed"),
        ping_status: PingStatus::new("closed"),
        password: b"secret".to_vec(),
        name: slug("%e2%9c%93-full"),
        to_ping: b"http://a.example/".to_vec(),
        pinged: b"http://b.example/".to_vec(),
        modified: PostDate {
            local: PostDateTime::new("2024-02-06 11:21:31"),
            gmt: PostDateTime::new("2024-02-06 16:21:31"),
        },
        content_filtered: b"filtered".to_vec(),
        parent: b,
        guid: b"http://example.org/?p=1".to_vec(),
        menu_order: -3,
        post_type: post_type("page"),
        mime_type: MimeType::from("image/png"),
        comment_count: 0,
    };
    let full_id = insert(&full);
    let read = find(full_id);
    let expected = Post {
        id: full_id,
        ..full.clone()
    };
    ensure(
        "find_returns_what_insert_wrote",
        read.as_ref() == Some(&expected),
        || format!("{read:?}"),
    );

    // insert_does_not_write_comment_count
    let counted = insert(&Post {
        comment_count: 5,
        ..named("counted", "post", 0)
    });
    let count = find(counted).map(|p| p.comment_count);
    ensure(
        "insert_does_not_write_comment_count",
        count == Some(0),
        || format!("{count:?}"),
    );

    // find_missing_is_none
    let missing = find(PostId(9_999));
    let zero = find(PostId(0));
    ensure(
        "find_missing_is_none",
        missing.is_none() && zero.is_none(),
        || format!("{missing:?} {zero:?}"),
    );

    // attachment_owner_spans_types_and_skips_except
    let shared_post = insert(&named("shared", "post", 0));
    insert(&named("shared", "page", 4));
    let any = repo
        .attachment_slug_owner(&slug("shared"), PostId(0))
        .expect("attachment_slug_owner");
    let past_one = repo
        .attachment_slug_owner(&slug("shared"), shared_post)
        .expect("attachment_slug_owner");
    let nobody = repo
        .attachment_slug_owner(&slug("unshared"), PostId(0))
        .expect("attachment_slug_owner");
    ensure(
        "attachment_owner_spans_types_and_skips_except",
        any == Some(slug("shared")) && past_one == Some(slug("shared")) && nobody.is_none(),
        || format!("{any:?} {past_one:?} {nobody:?}"),
    );
    let only = insert(&named("lonely", "book", 0));
    let only_except = repo
        .attachment_slug_owner(&slug("lonely"), only)
        .expect("attachment_slug_owner");
    ensure(
        "attachment_owner_spans_types_and_skips_except",
        only_except.is_none(),
        || format!("{only_except:?}"),
    );

    // hierarchical_owner_matches_type_or_attachment_under_the_parent
    let hier = |name: &str, kind: &str, except: PostId, parent: u64| {
        repo.hierarchical_slug_owner(&slug(name), &post_type(kind), except, PostId(parent))
            .expect("hierarchical_slug_owner")
    };
    let chapter = insert(&named("chapter", "page", 50));
    insert(&named("image", "attachment", 50));
    insert(&named("note", "post", 50));
    let same = hier("chapter", "page", PostId(0), 50);
    let attachment = hier("image", "page", PostId(0), 50);
    let other_type = hier("note", "page", PostId(0), 50);
    let other_parent = hier("chapter", "page", PostId(0), 51);
    let excepted = hier("chapter", "page", chapter, 50);
    ensure(
        "hierarchical_owner_matches_type_or_attachment_under_the_parent",
        same == Some(slug("chapter"))
            && attachment == Some(slug("image"))
            && other_type.is_none()
            && other_parent.is_none()
            && excepted.is_none(),
        || format!("{same:?} {attachment:?} {other_type:?} {other_parent:?} {excepted:?}"),
    );

    // flat_owner_matches_the_type_only
    let flat = |name: &str, kind: &str, except: PostId| {
        repo.flat_slug_owner(&slug(name), &post_type(kind), except)
            .expect("flat_slug_owner")
    };
    let story = insert(&named("story", "book", 60));
    insert(&named("cover", "attachment", 0));
    let same_type = flat("story", "book", PostId(0));
    let any_parent = flat("story", "book", PostId(0));
    let wrong_type = flat("story", "post", PostId(0));
    let not_attachment = flat("cover", "book", PostId(0));
    let skipped = flat("story", "book", story);
    ensure(
        "flat_owner_matches_the_type_only",
        same_type == Some(slug("story"))
            && any_parent == Some(slug("story"))
            && wrong_type.is_none()
            && not_attachment.is_none()
            && skipped.is_none(),
        || format!("{same_type:?} {wrong_type:?} {not_attachment:?} {skipped:?}"),
    );

    // owner_returns_the_stored_name: PHP truth is the rule's to test, so "0" and "" come back as stored.
    insert(&named("0", "post", 0));
    insert(&named("", "post", 0));
    let zero_name = repo
        .flat_slug_owner(&slug("0"), &post_type("post"), PostId(0))
        .expect("flat_slug_owner");
    let empty_name = repo
        .flat_slug_owner(&slug(""), &post_type("post"), PostId(0))
        .expect("flat_slug_owner");
    let zero_any = repo
        .attachment_slug_owner(&slug("0"), PostId(0))
        .expect("attachment_slug_owner");
    let empty_hier = repo
        .hierarchical_slug_owner(&slug(""), &post_type("post"), PostId(0), PostId(0))
        .expect("hierarchical_slug_owner");
    ensure(
        "owner_returns_the_stored_name",
        zero_name == Some(slug("0"))
            && empty_name == Some(slug(""))
            && zero_any == Some(slug("0"))
            && empty_hier == Some(slug("")),
        || format!("{zero_name:?} {empty_name:?} {zero_any:?} {empty_hier:?}"),
    );
}

/// Checks for repositories whose columns are binary-safe (memory, latin1 or binary tables); `post_repository_suite`
/// leaves them out. On stock utf8mb4 tables `wpdb::process_fields()` strips invalid text and `$wpdb->insert` returns
/// false, so a faithful SQL repository answers `Ok(None)` from `insert` and fails this suite by design. Like
/// `post_repository_suite`, it needs an empty table.
pub fn binary_safe_suite(repo: &dyn PostRepository) {
    // content_and_name_keep_invalid_utf8
    let raw_name = Slug::new(vec![b's', 0x00, 0xC2, 0xA0, 0xFF]);
    let raw_content = vec![0x00, 0xFF, 0xFE, b'c', 0xC3];
    let post = Post {
        name: raw_name.clone(),
        content: raw_content.clone(),
        ..Post::default()
    };
    let id = repo
        .insert(&post)
        .expect("insert")
        .expect("the insert returns an id");
    let read = repo.find(id).expect("find");
    let owner = repo
        .flat_slug_owner(&raw_name, &PostTypeName::new("post"), PostId(0))
        .expect("flat_slug_owner");
    ensure(
        "content_and_name_keep_invalid_utf8",
        read == Some(Post { id, ..post }) && owner == Some(raw_name.clone()),
        || format!("{read:?} {owner:?}"),
    );
}
