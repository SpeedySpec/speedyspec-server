use std::fs;
use std::path::Path;

use speedyspec_server_domain::{CacheRule, StaticSection};
use speedyspec_server_static::{StaticBuildError, StaticSet};

fn section(not_found: Option<&str>, rules: &[(&str, &str)]) -> StaticSection {
    StaticSection {
        root: "dist".to_string(),
        maintenance_page: None,
        not_found: not_found.map(str::to_string),
        cache: rules
            .iter()
            .map(|(pattern, value)| CacheRule {
                pattern: pattern.to_string(),
                cache_control: value.to_string(),
            })
            .collect(),
    }
}

fn spec_rules() -> StaticSection {
    section(
        None,
        &[
            ("_astro/**", "public, max-age=31536000, immutable"),
            ("**/*.html", "no-cache"),
        ],
    )
}

fn write(release: &Path, rel: &str, bytes: &[u8]) {
    let path = release.join("dist").join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn css(len: usize) -> Vec<u8> {
    b"body { color: red; }\n"
        .iter()
        .copied()
        .cycle()
        .take(len)
        .collect()
}

#[test]
fn builds_types_etags_and_cache_rules() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "index.html", b"<h1>hi</h1>");
    write(dir.path(), "_astro/app.4f2a.js", b"console.log(1)");
    write(dir.path(), "logo.png", b"\x89PNG....");
    write(dir.path(), "copy.png", b"\x89PNG....");

    let set = StaticSet::build(dir.path(), &spec_rules(), u64::MAX).unwrap();

    let index = set.file("index.html").unwrap();
    assert_eq!(index.content_type, "text/html; charset=utf-8");
    assert_eq!(index.cache_control, "no-cache");
    let js = set.file("_astro/app.4f2a.js").unwrap();
    assert_eq!(js.cache_control, "public, max-age=31536000, immutable");
    assert!(
        js.content_type.starts_with("text/javascript")
            || js.content_type.starts_with("application/javascript")
    );
    assert!(js.content_type.ends_with("; charset=utf-8"));
    let logo = set.file("logo.png").unwrap();
    assert_eq!(logo.content_type, "image/png");
    assert_eq!(logo.cache_control, "no-cache");
    for file in [index, js, logo] {
        let etag = &file.etag;
        assert_eq!(etag.len(), 34, "{etag}");
        assert!(etag.starts_with('"') && etag.ends_with('"'));
        assert!(
            etag[1..33]
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "{etag}"
        );
    }
    assert_eq!(logo.etag, set.file("copy.png").unwrap().etag);
    assert_ne!(logo.etag, index.etag);
}

#[test]
fn compresses_only_large_compressible() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "big.css", &css(4096));
    write(dir.path(), "small.css", &css(512));
    write(dir.path(), "big.png", &css(4096));

    let set = StaticSet::build(dir.path(), &spec_rules(), u64::MAX).unwrap();

    let big = set.file("big.css").unwrap();
    assert!(big.br.as_ref().unwrap().len() < big.identity.len());
    assert!(big.gzip.as_ref().unwrap().len() < big.identity.len());
    let small = set.file("small.css").unwrap();
    assert!(small.br.is_none() && small.gzip.is_none());
    let png = set.file("big.png").unwrap();
    assert!(png.br.is_none() && png.gzip.is_none());
}

#[test]
fn total_bytes_counts_every_variant() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "big.css", &css(4096));
    let set = StaticSet::build(dir.path(), &spec_rules(), u64::MAX).unwrap();
    let file = set.file("big.css").unwrap();
    let expected =
        file.identity.len() + file.br.as_ref().unwrap().len() + file.gzip.as_ref().unwrap().len();
    assert_eq!(set.total_bytes(), expected as u64);
}

#[test]
fn limit_exceeded() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "big.css", &css(4096));
    match StaticSet::build(dir.path(), &spec_rules(), 100) {
        Err(StaticBuildError::LimitExceeded { limit, needed }) => {
            assert_eq!(limit, 100);
            assert!(needed >= 4096, "{needed}");
        }
        other => panic!("expected LimitExceeded, got {:?}", other.err()),
    }
}

#[test]
fn errors() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        StaticSet::build(dir.path(), &spec_rules(), u64::MAX),
        Err(StaticBuildError::RootMissing(_))
    ));

    write(dir.path(), "index.html", b"x");
    let missing_404 = section(Some("404.html"), &[]);
    assert!(matches!(
        StaticSet::build(dir.path(), &missing_404, u64::MAX),
        Err(StaticBuildError::NotFoundMissing(name)) if name == "404.html"
    ));

    let bad = section(None, &[("[", "no-cache")]);
    assert!(matches!(
        StaticSet::build(dir.path(), &bad, u64::MAX),
        Err(StaticBuildError::BadPattern(_))
    ));

    std::os::unix::fs::symlink("/etc/passwd", dir.path().join("dist/link")).unwrap();
    assert!(matches!(
        StaticSet::build(dir.path(), &spec_rules(), u64::MAX),
        Err(StaticBuildError::UnsupportedFile(_))
    ));
}

#[test]
fn keeps_unicode_names() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "über menü.html", b"umlaut");
    write(dir.path(), "a+b.css", b"plus");
    let set = StaticSet::build(dir.path(), &spec_rules(), u64::MAX).unwrap();
    assert_eq!(&set.file("über menü.html").unwrap().identity[..], b"umlaut");
    assert_eq!(&set.file("a+b.css").unwrap().identity[..], b"plus");
}

#[test]
fn limit_checked_before_reading() {
    // An unreadable file over the limit: checking its size first gives LimitExceeded; reading first would fail with Io.
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "huge.bin", &css(4096));
    let path = dir.path().join("dist/huge.bin");
    std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o000)).unwrap();
    let result = StaticSet::build(dir.path(), &spec_rules(), 100);
    std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o644)).unwrap();
    assert!(
        matches!(
            result,
            Err(StaticBuildError::LimitExceeded { limit: 100, .. })
        ),
        "{:?}",
        result.err()
    );
}

#[test]
fn immutable_only_keeps_immutable_files() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "index.html", b"<p>x</p>");
    write(dir.path(), "_astro/app.4f2a.js", b"js");
    write(dir.path(), "404.html", b"nf");
    let mut rules = spec_rules();
    rules.not_found = Some("404.html".to_string());
    let set = StaticSet::build(dir.path(), &rules, u64::MAX).unwrap();
    let only = set.immutable_only();
    assert!(only.file("_astro/app.4f2a.js").is_some());
    assert!(only.file("index.html").is_none() && only.file("404.html").is_none());
    assert!(only.total_bytes() < set.total_bytes());
}
