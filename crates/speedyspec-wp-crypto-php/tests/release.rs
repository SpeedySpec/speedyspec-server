//! The release copy WordPress's tests run beside the native functions: functions.php and wp-admin/includes/file.php
//! lose exactly the ported functions, and every other file is the release's own.

use speedyspec_wp_crypto_php::{PORTED_FUNCTIONS, prepare_release, strip_ported};

fn declared(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| l.strip_prefix("function "))
        .map(|l| l.split('(').next().unwrap().trim().to_string())
        .collect()
}

#[test]
fn strips_exactly_the_ported_functions() {
    let release = speedyspec_compat::wordpress_release(&speedyspec_compat::pinned_wordpress())
        .expect("the pinned release");
    for file in ["wp-includes/functions.php", "wp-admin/includes/file.php"] {
        let source = std::fs::read_to_string(release.join(file)).unwrap();
        let stripped = strip_ported(file, &source).unwrap();
        let ported: Vec<&str> = PORTED_FUNCTIONS
            .iter()
            .filter(|(f, _)| *f == file)
            .map(|(_, n)| *n)
            .collect();
        assert_eq!(ported.len(), 2, "{file}");
        let before = declared(&source);
        let after = declared(&stripped);
        for name in &ported {
            assert!(before.iter().any(|n| n == name), "{file} declares {name}");
            assert!(
                !after.iter().any(|n| n == name),
                "{file} still declares {name}"
            );
        }
        let kept: Vec<&String> = before
            .iter()
            .filter(|n| !ported.contains(&n.as_str()))
            .collect();
        assert_eq!(
            after.iter().collect::<Vec<_>>(),
            kept,
            "{file} keeps everything else"
        );
        assert!(
            !stripped.contains("Returns a cryptographically secure hash")
                || file != "wp-includes/functions.php"
        );
    }
}

#[test]
fn prepared_release_links_everything_else() {
    let release = speedyspec_compat::wordpress_release(&speedyspec_compat::pinned_wordpress())
        .expect("the pinned release");
    let out = std::env::temp_dir().join(format!(
        "speedyspec-wp-crypto-release-{}",
        std::process::id()
    ));
    let prepared = prepare_release(&release, &out).unwrap();
    for (file, name) in PORTED_FUNCTIONS {
        let text = std::fs::read_to_string(prepared.join(file)).unwrap();
        assert!(
            !text.contains(&format!("\nfunction {name}(")),
            "{file}: {name}"
        );
    }
    for file in [
        "wp-includes/plugin.php",
        "wp-admin/includes/plugin.php",
        "wp-includes/class-wp-error.php",
        "index.php",
    ] {
        assert_eq!(
            std::fs::read(prepared.join(file)).unwrap(),
            std::fs::read(release.join(file)).unwrap(),
            "{file}"
        );
    }
    std::fs::remove_dir_all(&out).ok();
}
