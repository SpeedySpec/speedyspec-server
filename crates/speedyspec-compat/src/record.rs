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

/// `compat/wordpress.json`, the one release table: the repository's pin and every release it may name, with the URL
/// and SHA-256 each is fetched and checked by. Moving to a new release adds its entry here and changes `wordpress`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Releases {
    pub wordpress: String,
    pub releases: BTreeMap<String, Release>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Release {
    pub url: String,
    pub sha256: String,
}

/// `compat/<area>/source.json`: an area's optional override of the repository's pin. Without one (the file is `{}`,
/// or absent) the area is recorded from, and replays, the release `compat/wordpress.json` pins.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Source {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wordpress: Option<String>,
}

/// The largest release tarball the recorder accepts.
const LIMIT: u64 = 256 * 1024 * 1024;

/// The release table, `compat/wordpress.json`.
pub fn releases() -> Result<Releases, String> {
    read_json(&compat_dir().join("wordpress.json"))
}

/// `version`'s entry in the release table.
fn release(version: &str) -> Result<Release, String> {
    let mut table = releases()?;
    table.releases.remove(version).ok_or_else(|| {
        format!(
            "WordPress {version} is not in {}",
            compat_dir().join("wordpress.json").display()
        )
    })
}

/// The WordPress release the repository is pinned to: `compat/wordpress.json`'s `wordpress`. Areas without a
/// `source.json` override, and tests that load a release outside any area (abilities, REST, crypto, i18n, ...), run
/// against this one.
pub fn pinned_wordpress() -> String {
    releases().unwrap_or_else(|e| panic!("{e}")).wordpress
}

/// The WordPress release `area`'s fixture is recorded from and [`crate::load`] reads: `compat/<area>/source.json`'s
/// `wordpress` override, else the repository's pin.
pub fn area_wordpress(area: &str) -> String {
    try_area_wordpress(area).unwrap_or_else(|e| panic!("{e}"))
}

pub(crate) fn try_area_wordpress(area: &str) -> Result<String, String> {
    let path = compat_dir().join(area).join("source.json");
    let source: Source = if path.exists() {
        read_json(&path)?
    } else {
        Source::default()
    };
    match source.wordpress {
        Some(version) => Ok(version),
        None => Ok(releases()?.wordpress),
    }
}

/// WordPress `version` from the release table (`compat/wordpress.json`), unpacked once into the system's temporary
/// directory and reused; its `wordpress/` root.
pub fn wordpress_release(version: &str) -> Result<PathBuf, String> {
    unpacked(version, &release(version)?)
}

/// The release's tarball: `SPEEDYSPEC_COMPAT_TARBALL` when set, else fetched; checked against the pin either way.
fn tarball(release: &Release) -> Result<Vec<u8>, String> {
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
    Ok(tarball)
}

/// Unpacks the release once per pin (the directory is named by its SHA-256), atomically: a half-unpacked copy is
/// never reused.
fn unpacked(version: &str, release: &Release) -> Result<PathBuf, String> {
    let cache = std::env::temp_dir().join("speedyspec-compat");
    let dir = cache.join(format!(
        "wordpress-{version}-{}",
        &release.sha256[..16.min(release.sha256.len())]
    ));
    let root = dir.join("wordpress");
    if root.join("wp-includes/version.php").exists() {
        return Ok(root);
    }
    std::fs::create_dir_all(&cache).map_err(|e| format!("{}: {e}", cache.display()))?;
    let tarball = tarball(release)?;
    let staging = tempfile::tempdir_in(&cache).map_err(|e| e.to_string())?;
    let mut gz = flate2::read::GzDecoder::new(tarball.as_slice());
    let mut tar = Vec::new();
    gz.read_to_end(&mut tar)
        .map_err(|e| format!("{}: {e}", release.url))?;
    tar::Archive::new(tar.as_slice())
        .unpack(staging.path())
        .map_err(|e| format!("unpacking: {e}"))?;
    let staged = staging.keep();
    if std::fs::rename(&staged, &dir).is_err() && !root.join("wp-includes/version.php").exists() {
        return Err(format!(
            "{}: the unpacked release could not be moved into place",
            dir.display()
        ));
    }
    let _ = std::fs::remove_dir_all(&staged);
    Ok(root)
}

/// Records `area` against WordPress `version` and writes `compat/<area>/wordpress-<version>.json`.
pub fn record(area: &str, version: &str) -> Result<PathBuf, String> {
    record_for(area, version, false)
}

/// Records `area`'s single-site scenarios (`multisite` false) into `wordpress-<version>.json`, or its scenarios
/// marked `multisite` on a multisite install into `wordpress-<version>-multisite.json`.
pub fn record_for(area: &str, version: &str, multisite: bool) -> Result<PathBuf, String> {
    if multisite && area != "options" {
        return Err(format!(
            "the recorder has no multisite install for the area {area}"
        ));
    }
    let dir = compat_dir().join(area);
    let release = release(version)?;
    let mut catalogue: Catalogue = read_json(&dir.join("scenarios.json"))?;
    catalogue.scenarios.retain(|s| s.multisite == multisite);

    let mut runner = runner_for(area, version, &release, multisite)?;
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
    let path = dir.join(crate::fixture_name(version, multisite));
    let mut text = serde_json::to_string_pretty(&fixture).map_err(|e| e.to_string())?;
    text.push('\n');
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

/// A runner of `area`'s scenarios against WordPress `version`'s original code (single site), as the recorder runs
/// them, for tests that need to see what WordPress does with a hand-made scenario.
pub fn area_runner(area: &str, version: &str) -> Result<PhpRunner, String> {
    runner_for(area, version, &release(version)?, false)
}

fn runner_for(
    area: &str,
    version: &str,
    release: &Release,
    multisite: bool,
) -> Result<PhpRunner, String> {
    let root = unpacked(version, release)?;
    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas: Vec::new(),
    })
    .map_err(|e| e.to_string())?;
    let setup = setup_for(area, &root, multisite)?;
    Ok(PhpRunner::new(php, setup))
}

/// What prepares a fresh request for an area's scenarios.
type Setup = Box<dyn Fn(&Php) -> Result<(), String>>;

fn setup_for(area: &str, root: &Path, multisite: bool) -> Result<Setup, String> {
    let files: &[&str] = match area {
        "hooks" => &["wp-includes/plugin.php"],
        "wp-db" => &["wp-includes/plugin.php", "wp-includes/class-wpdb.php"],
        "object-cache" => &[
            "wp-includes/class-wp-object-cache.php",
            "wp-includes/cache.php",
        ],
        "options" | "formatting" | "shortcode" | "rewrite" | "kses" | "theme-json" | "cron" => &[],
        other => return Err(format!("the recorder does not know the area {other}")),
    };
    if matches!(
        area,
        "options" | "formatting" | "shortcode" | "rewrite" | "kses" | "theme-json" | "cron"
    ) {
        // `setup.php` loads WordPress's files itself, for either side.
        let code = format!(
            "define('SPEEDYSPEC_COMPAT_WP_ROOT', {:?}); define('SPEEDYSPEC_COMPAT_PORT', false); \
             define('SPEEDYSPEC_COMPAT_MULTISITE', {multisite}); require {:?};",
            format!("{}/", root.display()),
            compat_dir()
                .join(area)
                .join("setup.php")
                .display()
                .to_string()
        );
        return Ok(Box::new(move |php: &Php| {
            php.eval("speedyspec://compat/setup", &code)
                .map(|_| ())
                .map_err(|e| e.to_string())
        }));
    }
    let abspath = format!("{}/", root.display());
    let mut code = format!("define('ABSPATH', {abspath:?}); define('WPINC', 'wp-includes');");
    for file in files {
        code.push_str(&format!(" require ABSPATH . {file:?};"));
    }
    let support = compat_dir().join(area).join("support.php");
    if support.exists() {
        code.push_str(&format!(" require {:?};", support.display().to_string()));
    }
    // The stubs first: WordPress's cache calls them.
    let stubs = area == "object-cache";
    if stubs {
        code.push_str(" wp_cache_init();");
    }
    Ok(Box::new(move |php: &Php| {
        if stubs {
            php.eval(
                "speedyspec://compat/object-cache-stubs",
                crate::OBJECT_CACHE_STUBS,
            )
            .map_err(|e| e.to_string())?;
        }
        php.eval("speedyspec://compat/setup", &code)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every area resolves to a release the table pins, and has that release's fixture.
    #[test]
    fn every_area_resolves_to_a_pinned_release_with_a_fixture() {
        let table = releases().expect("compat/wordpress.json");
        assert!(
            table.releases.contains_key(&table.wordpress),
            "compat/wordpress.json pins {}, which is not in its releases",
            table.wordpress
        );
        for entry in std::fs::read_dir(compat_dir()).expect("compat/") {
            let dir = entry.expect("compat/ entry").path();
            if !dir.join("source.json").exists() {
                continue;
            }
            let area = dir.file_name().unwrap().to_string_lossy().into_owned();
            let version = area_wordpress(&area);
            assert!(
                table.releases.contains_key(&version),
                "{area} is recorded from WordPress {version}, which compat/wordpress.json does not list"
            );
            let fixture = dir.join(crate::fixture_name(&version, false));
            assert!(fixture.exists(), "{} is missing", fixture.display());
        }
    }
}
