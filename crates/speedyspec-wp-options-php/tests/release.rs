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
        "get_transient",
        "set_transient",
        "get_network_option",
        "update_site_option",
        "register_setting",
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
