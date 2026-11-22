use speedyspec_wp_cron_php::{PORTED_FUNCTIONS, strip_ported};

#[test]
fn strips_only_ported() {
    let release = speedyspec_compat::wordpress_release("7.1.5").unwrap();
    let source = std::fs::read_to_string(release.join("wp-includes/cron.php")).unwrap();
    let stripped = strip_ported(&source).unwrap();
    for name in PORTED_FUNCTIONS {
        assert!(!stripped.contains(&format!("\nfunction {name}(")), "{name}");
    }
    assert_eq!(PORTED_FUNCTIONS.len(), 17);
    assert!(
        !stripped.contains("\nfunction "),
        "every cron.php function is ported"
    );
}
