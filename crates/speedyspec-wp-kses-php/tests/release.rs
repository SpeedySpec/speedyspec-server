//! The release copy the native functions run beside: kses.php keeps its globals and the functions the port leaves to
//! PHP; functions.php loses only wp_allowed_protocols().

use speedyspec_wp_kses_php::{PORTED_FUNCTIONS, prepare_release, strip_ported};

fn declared(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| l.strip_prefix("function "))
        .map(|l| l.split('(').next().unwrap().trim().to_string())
        .collect()
}

#[test]
fn kses_keeps_only_what_stays_php() {
    let release = speedyspec_compat::wordpress_release("7.1.5").expect("the pinned release");
    let source = std::fs::read_to_string(release.join("wp-includes/kses.php")).unwrap();
    let stripped = strip_ported("kses.php", &source).unwrap();
    assert_eq!(
        declared(&stripped),
        vec![
            "_wp_kses_sanitize_note_mention_classes",
            "wp_filter_global_styles_post"
        ]
    );
    assert!(stripped.contains(
        "global $allowedposttags, $allowedtags, $allowedentitynames, $allowedxmlentitynames;"
    ));
    assert!(stripped.contains("$allowedentitynames = array("));
    for name in declared(&source) {
        let stays = [
            "_wp_kses_sanitize_note_mention_classes",
            "wp_filter_global_styles_post",
        ]
        .contains(&name.as_str());
        assert_eq!(PORTED_FUNCTIONS.contains(&name.as_str()), !stays, "{name}");
    }
}

#[test]
fn functions_loses_only_wp_allowed_protocols() {
    let release = speedyspec_compat::wordpress_release("7.1.5").expect("the pinned release");
    let source = std::fs::read_to_string(release.join("wp-includes/functions.php")).unwrap();
    let stripped = strip_ported("functions.php", &source).unwrap();
    let before = declared(&source);
    let after = declared(&stripped);
    assert_eq!(before.len() - after.len(), 1);
    assert!(!after.contains(&"wp_allowed_protocols".to_string()));
    assert!(PORTED_FUNCTIONS.contains(&"wp_allowed_protocols"));
}

#[test]
fn prepared_release_lints() {
    let release = speedyspec_compat::wordpress_release("7.1.5").expect("the pinned release");
    let out = tempfile::tempdir().unwrap();
    let root = prepare_release(&release, &out.path().join("wordpress")).unwrap();
    assert!(root.join("wp-includes/option.php").exists());
    let php = std::env::var("PHP").unwrap_or_else(|_| "php".into());
    for file in ["kses.php", "functions.php"] {
        let lint = std::process::Command::new(&php)
            .arg("-l")
            .arg(root.join("wp-includes").join(file))
            .output()
            .unwrap();
        assert!(
            lint.status.success(),
            "{file}: {}",
            String::from_utf8_lossy(&lint.stdout)
        );
    }
}

/// The parameter list of `function <name>(...)` in `text`, whitespace removed.
fn params(text: &str, name: &str) -> Option<String> {
    let line = text
        .lines()
        .find(|l| l.trim_start().starts_with(&format!("function {name}(")))?;
    let open = line.find('(')?;
    let close = line.rfind(')')?;
    Some(line[open + 1..close].split_whitespace().collect())
}

#[test]
fn facade_declares_every_ported_function_with_its_signature() {
    let release = speedyspec_compat::wordpress_release("7.1.5").expect("the pinned release");
    let kses = std::fs::read_to_string(release.join("wp-includes/kses.php")).unwrap();
    let functions = std::fs::read_to_string(release.join("wp-includes/functions.php")).unwrap();
    for name in PORTED_FUNCTIONS {
        let source = if *name == "wp_allowed_protocols" {
            &functions
        } else {
            &kses
        };
        assert_eq!(
            params(speedyspec_wp_kses_php::FACADE, name),
            params(source, name),
            "{name}"
        );
    }
}
