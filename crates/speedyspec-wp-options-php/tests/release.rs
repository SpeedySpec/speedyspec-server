//! The release copy the native functions run beside: `option.php` keeps every function the port does not declare.

use speedyspec_wp_options_php::{PORTED_FUNCTIONS, prepare_release, strip_ported};

#[test]
fn strips_exactly_the_ported_functions() {
    let release = speedyspec_compat::wordpress_release("7.1.5").expect("the pinned release");
    let source = std::fs::read_to_string(release.join("wp-includes/option.php")).unwrap();
    let stripped = strip_ported(&source).unwrap();
    for name in PORTED_FUNCTIONS {
        assert!(
            !stripped.contains(&format!("\nfunction {name}(")),
            "{name} is still declared"
        );
    }
    for kept in [
        "wp_user_settings",
        "get_user_setting",
        "set_user_setting",
        "delete_user_setting",
        "get_all_user_settings",
        "wp_set_all_user_settings",
        "delete_all_user_settings",
        "register_initial_settings",
    ] {
        assert!(
            stripped.contains(&format!("\nfunction {kept}(")),
            "{kept} was removed"
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
    let php = std::env::var("PHP").unwrap_or_else(|_| "php".into());
    let lint = std::process::Command::new(php)
        .arg("-l")
        .arg(root.join("wp-includes/option.php"))
        .output()
        .unwrap();
    assert!(
        lint.status.success(),
        "{}",
        String::from_utf8_lossy(&lint.stdout)
    );
}

#[test]
fn a_stale_prepared_copy_is_rewritten() {
    let release = speedyspec_compat::wordpress_release("7.1.5").expect("the pinned release");
    let source = std::fs::read_to_string(release.join("wp-includes/option.php")).unwrap();
    let out = tempfile::tempdir().unwrap();
    let root = prepare_release(&release, &out.path().join("wordpress")).unwrap();
    let option = root.join("wp-includes/option.php");
    // A copy prepared by a build that ported fewer functions.
    std::fs::write(&option, &source).unwrap();
    prepare_release(&release, &out.path().join("wordpress")).unwrap();
    assert_eq!(
        std::fs::read_to_string(&option).unwrap(),
        strip_ported(&source).unwrap()
    );
}
