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

const TRIPLES: [&str; 3] = [
    "x86_64-unknown-linux-musl",
    "aarch64-unknown-linux-musl",
    "aarch64-apple-darwin",
];

/// Every asset a complete release of `binary` has.
fn complete_assets(binary: &str) -> Vec<String> {
    let mut names: Vec<String> = TRIPLES
        .iter()
        .flat_map(|triple| {
            [
                format!("{binary}-{triple}"),
                format!("{binary}-{triple}.minisig"),
            ]
        })
        .collect();
    names.push("SHA256SUMS".into());
    names
}

/// What `release-publish-plan.sh` says for a release with these assets (as `gh release view --json assets` prints).
fn publish_plan(binary: &str, names: &[String]) -> String {
    let dir = tempfile::tempdir().unwrap();
    let assets: Vec<serde_json::Value> = names
        .iter()
        .map(|name| serde_json::json!({ "name": name, "size": 1 }))
        .collect();
    let file = dir.path().join("assets.json");
    std::fs::write(&file, serde_json::json!({ "assets": assets }).to_string()).unwrap();
    let output = run(
        "release-publish-plan.sh",
        &[binary, file.to_str().unwrap()],
        &[],
    );
    assert!(output.status.success(), "{output:?}");
    stdout(&output).trim().to_string()
}

#[test]
fn publish_complete_release_is_left_alone() {
    let mut names = complete_assets("speedyspec-server");
    names.push("notes.txt".into());
    assert_eq!(publish_plan("speedyspec-server", &names), "complete");
}

#[test]
fn publish_no_assets_is_empty() {
    assert_eq!(publish_plan("speedyspec-server", &[]), "empty");
}

#[test]
fn publish_one_missing_signature_is_incomplete() {
    let names: Vec<String> = complete_assets("speedyspec-server")
        .into_iter()
        .filter(|name| name != "speedyspec-server-aarch64-unknown-linux-musl.minisig")
        .collect();
    assert_eq!(publish_plan("speedyspec-server", &names), "incomplete");
}

#[test]
fn publish_missing_sums_is_incomplete() {
    let names: Vec<String> = complete_assets("speedyspec-server")
        .into_iter()
        .filter(|name| name != "SHA256SUMS")
        .collect();
    assert_eq!(publish_plan("speedyspec-server", &names), "incomplete");
}

#[test]
fn publish_another_binarys_assets_do_not_count() {
    assert_eq!(
        publish_plan(
            "speedyspec-server",
            &complete_assets("speedyspec-php-runtime")
                .into_iter()
                .filter(|name| name != "SHA256SUMS")
                .collect::<Vec<_>>()
        ),
        "empty"
    );
}

#[test]
fn publish_usage_exits_2() {
    assert_eq!(
        run("release-publish-plan.sh", &["speedyspec-server"], &[])
            .status
            .code(),
        Some(2)
    );
}

const DOWNLOAD: &str = "https://github.com/SpeedySpec/speedyspec-server/releases/download";

/// A GitHub release of `binary` at `tag` (as `gh api repos/.../releases` lists it), with every asset of a complete
/// release except `missing`, and the checksum file its `SHA256SUMS` would hold.
fn gh_release(
    binary: &str,
    tag: &str,
    missing: &[&str],
    draft: bool,
    prerelease: bool,
) -> (serde_json::Value, String) {
    let assets: Vec<serde_json::Value> = complete_assets(binary)
        .into_iter()
        .filter(|name| !missing.contains(&name.as_str()))
        .map(|name| {
            serde_json::json!({
                "name": name,
                "browser_download_url": format!("{DOWNLOAD}/{tag}/{name}"),
            })
        })
        .collect();
    let mut sums: Vec<String> = TRIPLES
        .iter()
        .map(|triple| format!("{}  {binary}-{triple}", sha(tag, triple)))
        .collect();
    sums.sort_by(|a, b| a[66..].cmp(&b[66..]));
    (
        serde_json::json!({
            "tag_name": tag,
            "draft": draft,
            "prerelease": prerelease,
            "published_at": "2026-11-20T12:00:00Z",
            "assets": assets,
        }),
        sums.join("\n") + "\n",
    )
}

/// A made-up checksum, distinct per release and target.
fn sha(tag: &str, triple: &str) -> String {
    let seed = format!("{tag}{triple}");
    let mut out = String::new();
    let mut n: u64 = seed
        .bytes()
        .fold(7, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
    while out.len() < 64 {
        n = n
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        out.push_str(&format!("{:016x}", n));
    }
    out.truncate(64);
    out
}

/// The fixture releases, written as `pages` JSON arrays one after the other (as `gh api --paginate` prints them),
/// and their `SHA256SUMS` files.
fn fixtures(pages: usize) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let releases = [
        gh_release(
            "speedyspec-server",
            "speedyspec-server-v0.1.0",
            &[],
            false,
            false,
        ),
        gh_release(
            "speedyspec-server",
            "speedyspec-server-v0.2.0",
            &[],
            false,
            false,
        ),
        gh_release(
            "speedyspec-server",
            "speedyspec-server-v0.3.0",
            &["speedyspec-server-aarch64-unknown-linux-musl.minisig"],
            false,
            false,
        ),
        gh_release(
            "speedyspec-server",
            "speedyspec-server-v0.4.0",
            &[],
            true,
            false,
        ),
        gh_release(
            "speedyspec-server",
            "speedyspec-server-v0.5.0-rc.1",
            &[],
            false,
            true,
        ),
        gh_release(
            "speedyspec-php-runtime",
            "speedyspec-php-runtime-v1.0.0",
            &[],
            false,
            false,
        ),
        gh_release("speedyspec-cli", "v9.9.9", &[], false, false),
        // A pre-release version GitHub was not told about.
        gh_release(
            "speedyspec-server",
            "speedyspec-server-v0.6.0-beta.1",
            &[],
            false,
            false,
        ),
    ];
    let sums = dir.path().join("sums");
    std::fs::create_dir(&sums).unwrap();
    let mut listed: Vec<serde_json::Value> = Vec::new();
    for (release, sum) in releases {
        let tag = release["tag_name"].as_str().unwrap().to_string();
        std::fs::write(sums.join(format!("{tag}.SHA256SUMS")), sum).unwrap();
        listed.push(release);
    }
    let per_page = listed.len().div_ceil(pages);
    let text: String = listed
        .chunks(per_page)
        .map(|page| serde_json::Value::Array(page.to_vec()).to_string() + "\n")
        .collect();
    std::fs::write(dir.path().join("releases.json"), text).unwrap();
    dir
}

fn index(
    dir: &Path,
    component: &str,
    app: &str,
) -> (speedyspec_server_domain::ReleaseIndex, String) {
    let releases = dir.join("releases.json");
    let sums = dir.join("sums");
    let output = run(
        "release-index.sh",
        &[component, app],
        &[
            ("RELEASE_INDEX_RELEASES", &releases),
            ("RELEASE_INDEX_SUMS_DIR", &sums),
        ],
    );
    assert!(output.status.success(), "{output:?}");
    let parsed = speedyspec_server_domain::ReleaseIndex::parse(&output.stdout)
        .unwrap_or_else(|err| panic!("{err}: {}", stdout(&output)));
    (parsed, String::from_utf8_lossy(&output.stderr).into_owned())
}

fn versions(index: &speedyspec_server_domain::ReleaseIndex) -> Vec<String> {
    index
        .releases
        .iter()
        .map(|entry| entry.version.to_string())
        .collect()
}

#[test]
fn index_lists_complete_releases_and_parses() {
    let dir = fixtures(1);
    let (parsed, _) = index(dir.path(), "speedyspec-server", "server");
    assert_eq!(parsed.app.as_str(), "server");
    assert_eq!(versions(&parsed), ["0.2.0", "0.1.0"]);
    let entry = &parsed.releases[0];
    assert_eq!(entry.published, "2026-11-20T12:00:00Z");
    let targets: Vec<&str> = entry.artifacts.iter().map(|a| a.target.as_str()).collect();
    assert_eq!(targets, TRIPLES);
    for artifact in &entry.artifacts {
        let name = format!("speedyspec-server-{}", artifact.target);
        assert_eq!(
            artifact.url,
            format!("{DOWNLOAD}/speedyspec-server-v0.2.0/{name}")
        );
        assert_eq!(
            artifact.minisig.as_deref(),
            Some(format!("{DOWNLOAD}/speedyspec-server-v0.2.0/{name}.minisig").as_str())
        );
        assert_eq!(
            artifact.sha256,
            sha("speedyspec-server-v0.2.0", &artifact.target)
        );
    }
}

#[test]
fn index_skips_incomplete_drafts_prereleases_and_other_components() {
    let dir = fixtures(1);
    let (parsed, stderr) = index(dir.path(), "speedyspec-server", "server");
    for absent in [
        "0.3.0",
        "0.4.0",
        "0.5.0-rc.1",
        "0.6.0-beta.1",
        "1.0.0",
        "9.9.9",
    ] {
        assert!(!versions(&parsed).contains(&absent.to_string()), "{absent}");
    }
    assert!(
        stderr.contains("speedyspec-server-v0.3.0")
            && stderr.contains("speedyspec-server-aarch64-unknown-linux-musl.minisig"),
        "{stderr}"
    );
}

#[test]
fn index_runtime_is_separate() {
    let dir = fixtures(1);
    let (parsed, _) = index(dir.path(), "speedyspec-php-runtime", "php-runtime");
    assert_eq!(parsed.app.as_str(), "php-runtime");
    assert_eq!(versions(&parsed), ["1.0.0"]);
}

#[test]
fn index_paged_releases_are_merged() {
    let dir = fixtures(8);
    let (parsed, _) = index(dir.path(), "speedyspec-server", "server");
    assert_eq!(versions(&parsed), ["0.2.0", "0.1.0"]);
}

#[test]
fn index_usage_exits_2() {
    assert_eq!(
        run("release-index.sh", &["speedyspec-server"], &[])
            .status
            .code(),
        Some(2)
    );
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read_json(path: &Path) -> serde_json::Value {
    serde_json::from_str(
        &std::fs::read_to_string(path).unwrap_or_else(|err| panic!("{}: {err}", path.display())),
    )
    .unwrap()
}

/// Every workspace crate, by directory (`crates/<name>`), with its name and version.
fn workspace_crates() -> Vec<(String, String, String)> {
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(repo_root())
        .output()
        .unwrap();
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let root = metadata["workspace_root"].as_str().unwrap().to_string();
    let mut crates: Vec<(String, String, String)> = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            let dir = Path::new(p["manifest_path"].as_str().unwrap())
                .parent()
                .unwrap()
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            (
                dir,
                p["name"].as_str().unwrap().to_string(),
                p["version"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    crates.sort();
    crates
}

const COMPONENTS: [&str; 2] = ["speedyspec-server", "speedyspec-php-runtime"];

/// Every crate is a release-please package, so a change in any crate is seen, and the `cargo-workspace` plugin bumps
/// the server or the runtime when a crate it is built from is bumped. Only those two publish GitHub releases.
#[test]
fn release_config_registers_every_crate() {
    let config = read_json(&repo_root().join("release-please-config.json"));
    let manifest = read_json(&repo_root().join(".release-please-manifest.json"));
    let packages = config["packages"].as_object().unwrap();
    let crates = workspace_crates();
    assert_eq!(packages.len(), crates.len(), "{:?}", packages.keys());
    for (dir, name, version) in &crates {
        let entry = &packages
            .get(dir)
            .unwrap_or_else(|| panic!("{dir} is not a release-please package"));
        assert_eq!(entry["component"], name.as_str(), "{dir}");
        assert!(entry.get("additional-paths").is_none(), "{dir}");
        if COMPONENTS.contains(&name.as_str()) {
            assert_eq!(
                entry["release-as"],
                version.as_str(),
                "{dir}: the first release"
            );
            assert!(entry.get("skip-github-release").is_none(), "{dir}");
            assert_eq!(manifest[dir], "0.0.0", "{dir}: never released");
        } else {
            assert_eq!(entry["skip-github-release"], true, "{dir}");
            assert_eq!(entry["skip-changelog"], true, "{dir}");
            assert_eq!(manifest[dir], version.as_str(), "{dir}");
        }
    }
    assert_eq!(config["release-type"], "rust");
    let plugin = config["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .find(|plugin| plugin["type"] == "cargo-workspace")
        .expect("the cargo-workspace plugin");
    assert_ne!(
        plugin["merge"], false,
        "one release pull request for every crate"
    );
    for component in COMPONENTS {
        assert!(
            pending(&repo_root(), component).starts_with("pending=false\n"),
            "{component}"
        );
    }
}

/// A repository with this release-please manifest and these tags.
fn manifest_repo(manifest: &serde_json::Value, tags: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".release-please-manifest.json"),
        manifest.to_string(),
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

fn tags(dir: &Path) -> Vec<String> {
    let output = Command::new("git")
        .args(["tag", "-l"])
        .current_dir(dir)
        .output()
        .unwrap();
    stdout(&output).lines().map(String::from).collect()
}

/// After a release, each library crate is tagged at its released version (with no GitHub release), so release-please
/// counts only its commits since then. The server and the runtime are tagged by release-please itself.
#[test]
fn library_tags_are_created_only_where_missing() {
    let dir = manifest_repo(
        &serde_json::json!({
            "crates/speedyspec-server": "0.1.0",
            "crates/speedyspec-php-runtime": "1.0.0",
            "crates/speedyspec-server-gate": "0.1.1",
            "crates/speedyspec-php": "0.2.0",
        }),
        &["speedyspec-php-v0.2.0"],
    );
    let output = run(
        "release-tag-libraries.sh",
        &["HEAD"],
        &[("SPEEDYSPEC_REPO", dir.path())],
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(stdout(&output), "speedyspec-server-gate-v0.1.1\n");
    assert_eq!(
        tags(dir.path()),
        ["speedyspec-php-v0.2.0", "speedyspec-server-gate-v0.1.1"]
    );
    let again = run(
        "release-tag-libraries.sh",
        &["HEAD"],
        &[("SPEEDYSPEC_REPO", dir.path())],
    );
    assert_eq!(stdout(&again), "", "nothing left to tag");
}

#[test]
fn library_tags_usage_exits_2() {
    assert_eq!(
        run("release-tag-libraries.sh", &[], &[]).status.code(),
        Some(2)
    );
}
