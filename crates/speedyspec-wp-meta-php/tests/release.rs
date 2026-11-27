//! The release copy the native functions run beside: `meta.php` keeps `get_meta_sql()` and loses the 23 ported
//! functions, and `class-wp-metadata-lazyloader.php` declares nothing.

use speedyspec_wp_meta_php::{
    LAZYLOADER_FILE, LAZYLOADER_STUB, PORTED_FUNCTIONS, prepare_release, strip_ported,
};

fn release() -> std::path::PathBuf {
    speedyspec_compat::wordpress_release(&speedyspec_compat::area_wordpress("metadata"))
        .expect("the pinned release")
}

#[test]
fn strips_exactly_the_ported_functions() {
    let release = release();
    let source = std::fs::read_to_string(release.join("wp-includes/meta.php")).unwrap();
    let stripped = strip_ported(&source).unwrap();
    assert_eq!(PORTED_FUNCTIONS.len(), 23);
    for name in PORTED_FUNCTIONS {
        assert!(
            !stripped.contains(&format!("\nfunction {name}(")),
            "{name} is still declared"
        );
    }
    assert!(
        stripped.contains("\nfunction get_meta_sql("),
        "get_meta_sql was removed"
    );
    assert!(
        stripped.contains("class-wp-metadata-lazyloader.php"),
        "the require was removed"
    );
    let declared = |text: &str| text.matches("\nfunction ").count();
    assert_eq!(
        declared(&source) - declared(&stripped),
        PORTED_FUNCTIONS.len()
    );
    // get_meta_sql() is the only function left.
    assert_eq!(declared(&stripped), 1);

    let out = tempfile::tempdir().unwrap();
    let root = prepare_release(&release, &out.path().join("wordpress")).unwrap();
    assert!(root.join("wp-includes/functions.php").exists());
    let class = std::fs::read_to_string(root.join(LAZYLOADER_FILE)).unwrap();
    assert_eq!(class, LAZYLOADER_STUB);
    assert!(!class.contains("class WP_Metadata_Lazyloader"));
    let php = std::env::var("PHP").unwrap_or_else(|_| "php".into());
    for file in ["wp-includes/meta.php", LAZYLOADER_FILE] {
        let lint = std::process::Command::new(&php)
            .arg("-l")
            .arg(root.join(file))
            .output()
            .unwrap();
        assert!(
            lint.status.success(),
            "{file}: {}",
            String::from_utf8_lossy(&lint.stdout)
        );
    }
}

#[test]
fn a_stale_prepared_copy_is_rewritten() {
    let release = release();
    let source = std::fs::read_to_string(release.join("wp-includes/meta.php")).unwrap();
    let out = tempfile::tempdir().unwrap();
    let root = prepare_release(&release, &out.path().join("wordpress")).unwrap();
    let meta = root.join("wp-includes/meta.php");
    std::fs::write(&meta, &source).unwrap();
    std::fs::write(root.join(LAZYLOADER_FILE), "<?php\n").unwrap();
    prepare_release(&release, &out.path().join("wordpress")).unwrap();
    assert_eq!(
        std::fs::read_to_string(&meta).unwrap(),
        strip_ported(&source).unwrap()
    );
    assert_eq!(
        std::fs::read_to_string(root.join(LAZYLOADER_FILE)).unwrap(),
        LAZYLOADER_STUB
    );
}

/// The facade declares every ported function with WordPress's own parameters and defaults.
#[test]
fn facade_declares_every_ported_function() {
    let facade = include_str!("../src/meta.php");
    let source = std::fs::read_to_string(release().join("wp-includes/meta.php")).unwrap();
    for name in PORTED_FUNCTIONS {
        let head = format!("function {name}(");
        let line = |text: &str| -> String {
            let at = text
                .find(&head)
                .unwrap_or_else(|| panic!("{name} is not declared"));
            let end = text[at..].find(')').unwrap() + at;
            text[at + head.len()..end].replace(' ', "")
        };
        assert_eq!(line(facade), line(&source), "{name}'s parameters differ");
    }
}
