//! The release step strips exactly the functions this crate provides natively from formatting.php.

use speedyspec_wp_text_php::{PORTED_FUNCTIONS, native_names, prepare_release, strip_ported};

/// Every `function name(` declared at the start of a line.
fn declared(source: &str) -> Vec<String> {
    source
        .lines()
        .filter_map(|l| l.strip_prefix("function "))
        .filter_map(|rest| rest.split('(').next())
        .map(|name| name.trim().to_string())
        .collect()
}

#[test]
fn ported_list_matches_natives() {
    let mut ported: Vec<&str> = PORTED_FUNCTIONS.to_vec();
    let mut natives = native_names();
    ported.sort_unstable();
    natives.sort_unstable();
    assert_eq!(ported, natives);
}

#[test]
fn strips_exactly_the_ported_functions() {
    let release = speedyspec_compat::wordpress_release(&speedyspec_compat::area_wordpress("formatting")).expect("the pinned release");
    let source = std::fs::read_to_string(release.join("wp-includes/formatting.php")).unwrap();
    let stripped = strip_ported(&source).unwrap();
    let before = declared(&source);
    let after = declared(&stripped);
    for name in PORTED_FUNCTIONS {
        assert!(before.iter().any(|n| n == name), "7.1.5's formatting.php declares {name}");
        assert!(!after.iter().any(|n| n == name), "{name} is stripped");
    }
    let kept: Vec<&String> = before.iter().filter(|n| !PORTED_FUNCTIONS.contains(&n.as_str())).collect();
    assert_eq!(after.iter().collect::<Vec<_>>(), kept, "every other function stays, in order");
    assert!(after.iter().any(|n| n == "sanitize_option") && after.iter().any(|n| n == "map_deep"));
}

#[test]
fn prepared_release_lints_and_keeps_the_kses_strip() {
    let release = speedyspec_compat::wordpress_release(&speedyspec_compat::area_wordpress("formatting")).expect("the pinned release");
    let out = tempfile::tempdir().unwrap();
    let root = prepare_release(&release, &out.path().join("wordpress")).unwrap();
    let formatting = root.join("wp-includes/formatting.php");
    assert!(!declared(&std::fs::read_to_string(&formatting).unwrap()).iter().any(|n| n == "esc_html"));
    let kses = std::fs::read_to_string(root.join("wp-includes/kses.php")).unwrap();
    assert!(!declared(&kses).iter().any(|n| n == "wp_kses"), "kses's own strip is kept");
    let php = std::env::var("PHP").unwrap_or_else(|_| "../../.php-build/buildroot/bin/php".into());
    let lint = std::process::Command::new(php).arg("-l").arg(&formatting).output().expect("php -l runs");
    assert!(lint.status.success(), "{}", String::from_utf8_lossy(&lint.stdout));
    // Preparing again reuses the copy.
    prepare_release(&release, &out.path().join("wordpress")).unwrap();
}
