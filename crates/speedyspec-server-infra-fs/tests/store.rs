use std::path::Path;

use semver::Version;
use speedyspec_server_domain::{AppId, ReleaseStore};
use speedyspec_server_infra_fs::FsReleaseStore;
use speedyspec_server_infra_memory::archive::{Entry, build, release};

const MANIFEST: &str =
    r#"{"app":"admin","version":"1.0.0","kind":"static","static":{"root":"dist"}}"#;

fn admin() -> AppId {
    AppId::parse("admin").unwrap()
}

fn v1() -> Version {
    Version::new(1, 0, 0)
}

struct Setup {
    outer: tempfile::TempDir,
    store: FsReleaseStore,
}

impl Setup {
    fn data(&self) -> std::path::PathBuf {
        self.outer.path().join("data")
    }

    /// Writes `bytes` as the downloaded artifact and unpacks it.
    fn unpack(&self, bytes: &[u8]) -> Result<speedyspec_server_domain::ReleaseManifest, String> {
        let partial = self.store.partial_dir(&admin(), &v1());
        std::fs::create_dir_all(&partial).unwrap();
        let artifact = partial.join("artifact");
        std::fs::write(&artifact, bytes).unwrap();
        self.store
            .unpack(&admin(), &v1(), &artifact)
            .map_err(|err| err.to_string())
    }

    /// Everything under the temp dir except the data dir's own releases.
    fn outside(&self) -> Vec<String> {
        fn walk(dir: &Path, out: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                out.push(path.display().to_string());
                if path.is_dir() && !path.is_symlink() {
                    walk(&path, out);
                }
            }
        }
        let mut out = Vec::new();
        walk(self.outer.path(), &mut out);
        out.retain(|p| !p.starts_with(&self.data().join("releases").display().to_string()));
        out
    }
}

fn setup() -> Setup {
    setup_with(None)
}

fn setup_with(limit: Option<u64>) -> Setup {
    let outer = tempfile::tempdir().unwrap();
    let data = outer.path().join("data");
    std::fs::create_dir_all(&data).unwrap();
    let store = match limit {
        Some(limit) => FsReleaseStore::with_limit(&data, limit),
        None => FsReleaseStore::new(&data),
    };
    Setup { outer, store }
}

#[test]
fn unpack_and_commit() {
    let s = setup();
    let manifest = s
        .unpack(&release(MANIFEST, &[("dist/index.html", "hi")]))
        .unwrap();
    assert_eq!(manifest.version, v1());
    let dir = s.store.commit(&admin(), &v1()).unwrap();
    assert_eq!(dir, s.data().join("releases/admin/1.0.0"));
    assert_eq!(
        std::fs::read_to_string(dir.join("dist/index.html")).unwrap(),
        "hi"
    );
    assert!(!s.store.partial_dir(&admin(), &v1()).exists());
    assert_eq!(s.store.manifest(&admin(), &v1()).unwrap().version, v1());
    assert_eq!(s.store.release_dir(&admin(), &v1()), dir);
}

#[test]
fn manifest_mismatch_refused() {
    let s = setup();
    let other_app = MANIFEST.replace("\"admin\"", "\"blog\"");
    let err = s.unpack(&release(&other_app, &[])).unwrap_err();
    assert!(err.contains("app"), "{err}");
    let s = setup();
    let other_version = MANIFEST.replace("1.0.0", "2.0.0");
    let err = s.unpack(&release(&other_version, &[])).unwrap_err();
    assert!(err.contains("version"), "{err}");
}

fn rejects(entries: &[Entry], needle: &str) {
    rejects_with(setup(), entries, needle)
}

fn rejects_with(s: Setup, entries: &[Entry], needle: &str) {
    let before = s.outside();
    let mut all = vec![Entry::File("speedyspec-release.json", MANIFEST.as_bytes())];
    all.extend(entries.iter().map(|e| match e {
        Entry::File(a, b) => Entry::File(a, b),
        Entry::Dir(a) => Entry::Dir(a),
        Entry::Symlink(a, b) => Entry::Symlink(a, b),
        Entry::HardLink(a, b) => Entry::HardLink(a, b),
        Entry::Fifo(a) => Entry::Fifo(a),
        Entry::RawPath(a, b) => Entry::RawPath(a, b),
        Entry::PaxGlobal => Entry::PaxGlobal,
    }));
    let err = s.unpack(&build(&all)).unwrap_err();
    assert!(err.contains(needle), "error {err:?} must name {needle:?}");
    assert!(
        !s.store.partial_dir(&admin(), &v1()).exists(),
        "partial must be removed"
    );
    let mut after = s.outside();
    after.retain(|p| !before.contains(p));
    assert!(after.is_empty(), "written outside the release: {after:?}");
}

#[test]
fn rejects_parent_dir() {
    rejects(&[Entry::RawPath("../escape.txt", b"x")], "../escape.txt");
}

#[test]
fn rejects_absolute() {
    rejects(
        &[Entry::RawPath("/tmp/speedyspec-abs.txt", b"x")],
        "/tmp/speedyspec-abs.txt",
    );
}

#[test]
fn rejects_escaping_symlink() {
    rejects(&[Entry::Symlink("dist/out", "../../../etc")], "dist/out");
    rejects(&[Entry::Symlink("dist/abs", "/etc/passwd")], "dist/abs");
}

#[test]
fn rejects_hard_link() {
    rejects(
        &[
            Entry::File("dist/a", b"x"),
            Entry::HardLink("dist/b", "dist/a"),
        ],
        "dist/b",
    );
}

#[test]
fn rejects_fifo() {
    rejects(&[Entry::Fifo("dist/pipe")], "dist/pipe");
}

#[test]
fn rejects_oversized() {
    rejects_with(
        setup_with(Some(1024)),
        &[Entry::File("dist/big", &[0u8; 4096])],
        "1024",
    );
}

#[test]
fn allows_internal_symlink() {
    let s = setup();
    s.unpack(&build(&[
        Entry::File("speedyspec-release.json", MANIFEST.as_bytes()),
        Entry::File("dist/v1/index.html", b"x"),
        Entry::Symlink("dist/latest", "v1"),
    ]))
    .unwrap();
}

#[test]
fn clean_partials_removes_leftovers() {
    let s = setup();
    let partial = s.store.partial_dir(&admin(), &v1());
    std::fs::create_dir_all(partial.join("unpacked")).unwrap();
    let cleaned = s.store.clean_partials().unwrap();
    assert_eq!(cleaned, [partial.clone()]);
    assert!(!partial.exists());
}

#[test]
fn remove_deletes_release() {
    let s = setup();
    s.unpack(&release(MANIFEST, &[])).unwrap();
    let dir = s.store.commit(&admin(), &v1()).unwrap();
    s.store.remove(&admin(), &v1()).unwrap();
    assert!(!dir.exists());
}

#[test]
fn discard_partial_is_quiet_when_missing() {
    let s = setup();
    s.store.discard_partial(&admin(), &v1());
}

#[test]
fn rejects_symlink_escaping_through_another_symlink() {
    let entries = [
        Entry::Dir("a/b/c"),
        Entry::Symlink("a/b/c/up", "../../.."),
        Entry::Symlink("esc", "a/b/c/up/../../../.."),
    ];
    rejects(&entries, "esc");
    let reversed = [
        Entry::Dir("a/b/c"),
        Entry::Symlink("esc", "a/b/c/up/../../../.."),
        Entry::Symlink("a/b/c/up", "../../.."),
    ];
    rejects(&reversed, "esc");
}

#[test]
fn accepts_pax_global_header() {
    let s = setup();
    s.unpack(&build(&[
        Entry::PaxGlobal,
        Entry::File("speedyspec-release.json", MANIFEST.as_bytes()),
        Entry::File("dist/index.html", b"x"),
    ]))
    .unwrap();
}

#[test]
fn commit_binary_sets_executable() {
    use std::os::unix::fs::PermissionsExt;
    let s = setup();
    let partial = s.store.partial_dir(&admin(), &v1());
    std::fs::create_dir_all(&partial).unwrap();
    std::fs::write(partial.join("artifact"), b"bin").unwrap();
    let path = s
        .store
        .commit_binary(&admin(), &v1(), &partial.join("artifact"))
        .unwrap();
    assert_eq!(
        path,
        s.data().join("releases/admin/1.0.0/speedyspec-server")
    );
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o755
    );
    assert!(!partial.exists());
}

#[test]
fn unpack_refuses_a_php_release_without_its_entry() {
    let blog = AppId::parse("blog").unwrap();
    let manifest = r#"{"app":"blog","version":"1.0.0","kind":"php","php":{"document_root":"public","entry":"index.php"}}"#;
    let s = setup();
    let unpack = |files: &[(&str, &str)]| {
        let partial = s.store.partial_dir(&blog, &v1());
        std::fs::create_dir_all(&partial).unwrap();
        let artifact = partial.join("artifact");
        std::fs::write(&artifact, release(manifest, files)).unwrap();
        s.store
            .unpack(&blog, &v1(), &artifact)
            .map_err(|err| err.to_string())
    };
    let err = unpack(&[("index.php", "<?php")]).unwrap_err();
    assert!(
        err.contains("public/index.php"),
        "the refusal names the entry: {err}"
    );
    assert!(unpack(&[("public/index.php", "<?php")]).is_ok());
}
