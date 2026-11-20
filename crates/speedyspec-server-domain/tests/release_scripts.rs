//! The release pipeline's scripts (`scripts/release-*.sh`), run against fixtures. The workflows only call them, so
//! every decision the pipeline makes is tested here.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn script(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts")
        .join(name)
}

fn run(name: &str, args: &[&str], env: &[(&str, &Path)]) -> Output {
    let mut command = Command::new(script(name));
    command.args(args);
    for (key, value) in env {
        command.env(key, value);
    }
    command.output().unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

/// A repository with the two components at these crate versions, this release-please manifest, and these tags.
fn repo(server: &str, runtime: &str, manifest: (&str, &str), tags: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (component, version) in [
        ("speedyspec-server", server),
        ("speedyspec-php-runtime", runtime),
    ] {
        let crate_dir = dir.path().join("crates").join(component);
        std::fs::create_dir_all(&crate_dir).unwrap();
        std::fs::write(
            crate_dir.join("Cargo.toml"),
            format!(
                "[package]\nname = \"{component}\"\nversion = \"{version}\"\n\n[dependencies]\nserde = {{ version = \"1.0.0\" }}\n"
            ),
        )
        .unwrap();
    }
    std::fs::write(
        dir.path().join(".release-please-manifest.json"),
        format!(
            r#"{{"crates/speedyspec-server": "{}", "crates/speedyspec-php-runtime": "{}"}}"#,
            manifest.0, manifest.1
        ),
    )
    .unwrap();
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "init"]);
    for tag in tags {
        git(dir.path(), &["tag", tag]);
    }
    dir
}

fn pending(dir: &Path, component: &str) -> String {
    let output = run(
        "release-pending.sh",
        &[component],
        &[("SPEEDYSPEC_REPO", dir)],
    );
    assert!(output.status.success(), "{output:?}");
    stdout(&output)
}

#[test]
fn pending_after_a_merged_release_pr() {
    let dir = repo(
        "0.2.0",
        "1.0.0",
        ("0.2.0", "1.0.0"),
        &["speedyspec-php-runtime-v1.0.0"],
    );
    assert_eq!(
        pending(dir.path(), "speedyspec-server"),
        "pending=true\ntag=speedyspec-server-v0.2.0\nversion=0.2.0\n"
    );
}

#[test]
fn not_pending_once_tagged() {
    let dir = repo(
        "0.2.0",
        "1.0.0",
        ("0.2.0", "1.0.0"),
        &["speedyspec-server-v0.2.0"],
    );
    assert!(pending(dir.path(), "speedyspec-server").starts_with("pending=false\n"));
}

#[test]
fn never_released_sentinel_is_not_pending() {
    let dir = repo("0.1.0", "1.0.0", ("0.0.0", "0.0.0"), &[]);
    for component in ["speedyspec-server", "speedyspec-php-runtime"] {
        assert!(
            pending(dir.path(), component).starts_with("pending=false\n"),
            "{component}"
        );
    }
}

#[test]
fn open_release_pr_is_not_pending() {
    let dir = repo("0.1.0", "1.0.0", ("0.2.0", "1.0.0"), &[]);
    assert!(pending(dir.path(), "speedyspec-server").starts_with("pending=false\n"));
}

#[test]
fn components_are_independent() {
    let dir = repo(
        "0.2.0",
        "1.1.0",
        ("0.2.0", "1.1.0"),
        &["speedyspec-server-v0.2.0"],
    );
    assert!(pending(dir.path(), "speedyspec-server").starts_with("pending=false\n"));
    assert_eq!(
        pending(dir.path(), "speedyspec-php-runtime"),
        "pending=true\ntag=speedyspec-php-runtime-v1.1.0\nversion=1.1.0\n"
    );
}

#[test]
fn unknown_component_exits_2() {
    let dir = repo("0.1.0", "1.0.0", ("0.0.0", "0.0.0"), &[]);
    let output = run(
        "release-pending.sh",
        &["speedyspec-cli"],
        &[("SPEEDYSPEC_REPO", dir.path())],
    );
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let output = run("release-pending.sh", &[], &[]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
}
