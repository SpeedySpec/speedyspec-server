//! The recorder: fetches the pinned WordPress release, runs the catalogue against its code and writes the fixture.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, Php};

use crate::{Catalogue, Fixture, PhpRunner, ScenarioRunner, compat_dir, read_json};

/// `compat/<area>/source.json`: the release the fixture is recorded from, and the pinned releases.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Source {
    pub wordpress: String,
    pub releases: BTreeMap<String, Release>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Release {
    pub url: String,
    pub sha256: String,
}

/// The largest release tarball the recorder accepts.
const LIMIT: u64 = 256 * 1024 * 1024;

/// Records `area` against WordPress `version` and writes `compat/<area>/wordpress-<version>.json`.
pub fn record(area: &str, version: &str) -> Result<PathBuf, String> {
    let dir = compat_dir().join(area);
    let source: Source = read_json(&dir.join("source.json"))?;
    let release = source.releases.get(version).ok_or_else(|| {
        format!(
            "WordPress {version} is not pinned in {}",
            dir.join("source.json").display()
        )
    })?;
    let catalogue: Catalogue = read_json(&dir.join("scenarios.json"))?;

    // A copy already on disk (`SPEEDYSPEC_COMPAT_TARBALL`) is used instead of fetching; the pin checks it the same.
    let tarball = match std::env::var_os("SPEEDYSPEC_COMPAT_TARBALL") {
        Some(path) => {
            std::fs::read(&path).map_err(|e| format!("{}: {e}", Path::new(&path).display()))?
        }
        None => {
            eprintln!("fetching {}", release.url);
            ureq::get(&release.url)
                .call()
                .map_err(|e| format!("{}: {e}", release.url))?
                .into_body()
                .with_config()
                .limit(LIMIT)
                .read_to_vec()
                .map_err(|e| format!("{}: {e}", release.url))?
        }
    };
    let digest = format!("{:x}", Sha256::digest(&tarball));
    if digest != release.sha256 {
        return Err(format!(
            "{}: SHA-256 {digest}, pinned {}",
            release.url, release.sha256
        ));
    }
    let unpacked = tempfile::tempdir().map_err(|e| e.to_string())?;
    let mut gz = flate2::read::GzDecoder::new(tarball.as_slice());
    let mut tar = Vec::new();
    gz.read_to_end(&mut tar)
        .map_err(|e| format!("{}: {e}", release.url))?;
    tar::Archive::new(tar.as_slice())
        .unpack(unpacked.path())
        .map_err(|e| format!("unpacking: {e}"))?;
    let root = unpacked.path().join("wordpress");

    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas: Vec::new(),
    })
    .map_err(|e| e.to_string())?;
    let setup = setup_for(area, &root)?;
    let mut runner = PhpRunner::new(php, setup);
    let mut results = BTreeMap::new();
    let mut errors = Vec::new();
    for scenario in &catalogue.scenarios {
        match runner.run(scenario) {
            Ok(steps) => {
                results.insert(scenario.name.clone(), steps);
            }
            Err(why) => errors.push(format!("{}: {why}", scenario.name)),
        }
    }
    if !errors.is_empty() {
        return Err(format!("the recorder failed:\n{}", errors.join("\n")));
    }
    let fixture = Fixture {
        wordpress: version.to_string(),
        results,
    };
    let path = dir.join(format!("wordpress-{version}.json"));
    let mut text = serde_json::to_string_pretty(&fixture).map_err(|e| e.to_string())?;
    text.push('\n');
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

/// What loads WordPress's implementation of `area` into a fresh request.
fn setup_for(
    area: &str,
    root: &Path,
) -> Result<impl Fn(&Php) -> Result<(), String> + 'static, String> {
    let files: &[&str] = match area {
        "hooks" => &["wp-includes/plugin.php"],
        "wp-db" => &["wp-includes/plugin.php", "wp-includes/class-wpdb.php"],
        other => return Err(format!("the recorder does not know the area {other}")),
    };
    let abspath = format!("{}/", root.display());
    let mut code = format!("define('ABSPATH', {abspath:?}); define('WPINC', 'wp-includes');");
    for file in files {
        code.push_str(&format!(" require ABSPATH . {file:?};"));
    }
    let support = compat_dir().join(area).join("support.php");
    if support.exists() {
        code.push_str(&format!(" require {:?};", support.display().to_string()));
    }
    Ok(move |php: &Php| {
        php.eval("speedyspec://compat/setup", &code)
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
}
