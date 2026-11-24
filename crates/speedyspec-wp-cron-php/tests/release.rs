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

#[test]
fn prepared_release_has_the_endpoint_and_no_wp_cron() {
    let release = speedyspec_compat::wordpress_release("7.1.5").unwrap();
    let out = tempfile::tempdir().unwrap();
    let prepared = speedyspec_wp_cron_php::prepare_release(&release, out.path()).unwrap();
    let endpoint =
        std::fs::read_to_string(prepared.join(speedyspec_wp_cron_php::CRON_ENDPOINT)).unwrap();
    assert!(endpoint.contains("SpeedySpec\\WpCron\\run_due_events"));
    assert!(endpoint.contains("SpeedySpec\\WpCron\\due_sites"));
    let wp_cron = prepared.join("wp-cron.php");
    assert!(
        !wp_cron.symlink_metadata().unwrap().file_type().is_symlink(),
        "a file of its own, not a link into the shared release"
    );
    let stub = std::fs::read_to_string(&wp_cron).unwrap();
    assert!(stub.contains("http_response_code(404)"), "{stub}");
    let original = std::fs::read_to_string(release.join("wp-cron.php")).unwrap();
    assert!(
        original.contains("DOING_CRON"),
        "the shared release's wp-cron.php is untouched"
    );
}
