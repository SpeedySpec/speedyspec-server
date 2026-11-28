//! The release copy the native post functions run beside: `post.php` loses the 74 ported functions (the registries,
//! the readers, and the writers with their default callbacks) and keeps the rest (the meta wrappers, the `WP_Query`
//! callers, `update_post_author_caches()`, the attachment functions, `wp_untrash_post_set_previous_status()`), and it
//! still parses.
//! The mirror strips every ported function, the writers included.

use speedyspec_wp_posts_php::{
    CLASS_FILE, GET_INSTANCE_BODY, PORTED_FUNCTIONS, prepare_release, strip_ported,
    swap_get_instance,
};

fn release() -> std::path::PathBuf {
    speedyspec_compat::wordpress_release(&speedyspec_compat::area_wordpress("posts-read"))
        .expect("the pinned release")
}

/// Functions of `post.php` that stay WordPress's own.
const KEPT: &[&str] = &[
    "get_posts",
    "get_children",
    "get_pages",
    "wp_get_recent_posts",
    "wp_get_post_terms",
    "get_post_meta",
    "add_post_meta",
    "update_post_meta",
    "delete_post_meta",
    "get_post_custom",
    "update_post_author_caches",
    "wp_get_attachment_metadata",
    "get_extended",
    "wp_unique_post_slug",
    "wp_set_post_categories",
    "wp_add_trashed_suffix_to_post_name_for_trashed_posts",
    "wp_insert_attachment",
    "wp_delete_attachment",
    "wp_untrash_post_set_previous_status",
];

/// The writers and their default callbacks, which the release copy must not declare.
const WRITERS: &[&str] = &[
    "set_post_type",
    "wp_insert_post",
    "wp_update_post",
    "wp_transition_post_status",
    "_transition_post_status",
    "_future_post_hook",
    "_publish_post_hook",
    "wp_check_post_hierarchy_for_loops",
    "stick_post",
    "unstick_post",
    "wp_delete_post",
    "_reset_front_page_settings_for_post",
    "_reset_privacy_policy_page_for_post",
    "wp_trash_post",
    "wp_untrash_post",
    "wp_publish_post",
    "check_and_publish_future_post",
    "add_ping",
    "wp_delete_auto_drafts",
];

#[test]
fn strips_exactly_the_ported_functions() {
    let release = release();
    let source = std::fs::read_to_string(release.join("wp-includes/post.php")).unwrap();
    let stripped = strip_ported(&source).unwrap();
    assert_eq!(PORTED_FUNCTIONS.len(), 74);
    for name in WRITERS {
        assert!(PORTED_FUNCTIONS.contains(name), "{name} is not ported");
    }
    for name in PORTED_FUNCTIONS {
        assert!(
            !stripped.contains(&format!("\nfunction {name}(")),
            "{name} is still declared"
        );
    }
    for name in KEPT {
        assert!(
            stripped.contains(&format!("\nfunction {name}(")),
            "{name} was removed"
        );
    }
    let declared = |text: &str| text.matches("\nfunction ").count();
    assert_eq!(
        declared(&source) - declared(&stripped),
        PORTED_FUNCTIONS.len()
    );

    let out = tempfile::tempdir().unwrap();
    let root = prepare_release(&release, &out.path().join("wordpress")).unwrap();
    assert!(root.join("wp-includes/functions.php").exists());
    assert_eq!(
        std::fs::read_to_string(root.join("wp-includes/post.php")).unwrap(),
        stripped
    );
    let php = std::env::var("PHP").unwrap_or_else(|_| "php".into());
    let lint = std::process::Command::new(&php)
        .arg("-l")
        .arg(root.join("wp-includes/post.php"))
        .output()
        .unwrap();
    assert!(
        lint.status.success(),
        "post.php: {}",
        String::from_utf8_lossy(&lint.stdout)
    );
}

#[test]
fn preparations_chain_in_one_copy() {
    let release = release();
    let out = tempfile::tempdir().unwrap();
    let dir = out.path().join("wordpress");
    speedyspec_wp_meta_php::prepare_release(&release, &dir).unwrap();
    let root = prepare_release(&release, &dir).unwrap();
    let meta = std::fs::read_to_string(root.join("wp-includes/meta.php")).unwrap();
    assert!(
        !meta.contains("\nfunction get_metadata("),
        "meta.php was linked back"
    );
    let post = std::fs::read_to_string(root.join("wp-includes/post.php")).unwrap();
    assert!(!post.contains("\nfunction get_post("));
    for name in WRITERS {
        assert!(
            !post.contains(&format!("\nfunction {name}(")),
            "the mirror still declares {name}"
        );
    }
    assert!(post.contains("\nfunction wp_delete_attachment("));
}

#[test]
fn a_stale_prepared_copy_is_rewritten() {
    let release = release();
    let source = std::fs::read_to_string(release.join("wp-includes/post.php")).unwrap();
    let out = tempfile::tempdir().unwrap();
    let root = prepare_release(&release, &out.path().join("wordpress")).unwrap();
    let post = root.join("wp-includes/post.php");
    std::fs::write(&post, &source).unwrap();
    prepare_release(&release, &out.path().join("wordpress")).unwrap();
    assert_eq!(
        std::fs::read_to_string(&post).unwrap(),
        strip_ported(&source).unwrap()
    );
}

/// The facades declare every ported function with WordPress's own parameters and defaults.
#[test]
fn facades_declare_every_ported_function() {
    let facade = [
        include_str!("../src/post-types.php"),
        include_str!("../src/posts.php"),
        include_str!("../src/post-writes.php"),
    ]
    .concat();
    let source = std::fs::read_to_string(release().join("wp-includes/post.php")).unwrap();
    for name in PORTED_FUNCTIONS {
        let head = format!("function {name}(");
        let line = |text: &str| -> String {
            let at = text
                .find(&format!("\n{head}"))
                .or_else(|| text.find(&format!(" {head}")))
                .unwrap_or_else(|| panic!("{name} is not declared"));
            let start = text[at..].find(&head).unwrap() + at;
            let end = text[start..].find("\n").unwrap() + start;
            text[start + head.len()..end]
                .trim_end_matches(" {")
                .trim_end_matches('{')
                .replace(' ', "")
        };
        assert_eq!(line(&facade), line(&source), "{name}'s parameters differ");
    }
}

/// The prepared release swaps `WP_Post::get_instance()`'s body for the native raw read and keeps the rest of the
/// class: the signature, the docblock, the other methods. The copy parses.
#[test]
fn get_instance_body_is_swapped() {
    let release = release();
    let source = std::fs::read_to_string(release.join(CLASS_FILE)).unwrap();
    let swapped = swap_get_instance(&source).unwrap();
    assert!(
        source.contains("$wpdb->get_row("),
        "the release reads wpdb itself"
    );
    assert!(
        !swapped.contains("$wpdb->get_row("),
        "the swapped class still reads wpdb"
    );
    assert!(swapped.contains(GET_INSTANCE_BODY));
    assert!(swapped.contains("\tpublic static function get_instance( $post_id ) {\n"));
    for method in ["__construct", "__isset", "__get", "filter", "to_array"] {
        assert!(
            swapped.contains(&format!("public function {method}(")),
            "{method} was removed"
        );
    }
    assert_eq!(
        source.matches("function ").count(),
        swapped.matches("function ").count()
    );
    let out = tempfile::tempdir().unwrap();
    let root = prepare_release(&release, &out.path().join("wordpress")).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join(CLASS_FILE)).unwrap(),
        swapped
    );
    let php = std::env::var("PHP").unwrap_or_else(|_| "php".into());
    let lint = std::process::Command::new(&php)
        .arg("-l")
        .arg(root.join(CLASS_FILE))
        .output()
        .unwrap();
    assert!(
        lint.status.success(),
        "class-wp-post.php: {}",
        String::from_utf8_lossy(&lint.stdout)
    );
}
