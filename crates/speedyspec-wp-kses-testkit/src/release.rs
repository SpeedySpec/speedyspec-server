//! The pinned WordPress release, fetched and unpacked once into the same cache `speedyspec-compat` uses.

use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

const LIMIT: u64 = 256 * 1024 * 1024;

/// The kses area's pin, `compat/kses/source.json`: the release every comparison is made against, its URL and its
/// SHA-256.
struct Pin {
    wordpress: String,
    url: String,
    sha256: String,
}

fn pin() -> Result<Pin, String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../compat/kses/source.json");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let source: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let field = |value: &serde_json::Value, name: &str| {
        value[name]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| format!("{}: no {name}", path.display()))
    };
    let wordpress = field(&source, "wordpress")?;
    let release = &source["releases"][wordpress.as_str()];
    Ok(Pin {
        url: field(release, "url")?,
        sha256: field(release, "sha256")?,
        wordpress,
    })
}

/// The release every comparison is made against: `compat/kses/source.json`'s `wordpress`.
pub fn wordpress() -> String {
    pin().unwrap_or_else(|e| panic!("{e}")).wordpress
}

/// The unpacked release's `wordpress/` root.
pub fn wordpress_release() -> Result<PathBuf, String> {
    let Pin {
        wordpress: version,
        url,
        sha256,
    } = pin()?;
    let (url, sha256) = (url.as_str(), sha256.as_str());
    let cache = std::env::temp_dir().join("speedyspec-compat");
    let dir = cache.join(format!(
        "wordpress-{version}-{}",
        &sha256[..16.min(sha256.len())]
    ));
    let root = dir.join("wordpress");
    if root.join("wp-includes/version.php").exists() {
        return Ok(root);
    }
    std::fs::create_dir_all(&cache).map_err(|e| format!("{}: {e}", cache.display()))?;
    let tarball = ureq::get(url)
        .call()
        .map_err(|e| format!("{url}: {e}"))?
        .into_body()
        .with_config()
        .limit(LIMIT)
        .read_to_vec()
        .map_err(|e| format!("{url}: {e}"))?;
    let digest = format!("{:x}", Sha256::digest(&tarball));
    if digest != sha256 {
        return Err(format!("{url}: SHA-256 {digest}, pinned {sha256}"));
    }
    let staging = tempfile::tempdir_in(&cache).map_err(|e| e.to_string())?;
    let mut tar = Vec::new();
    flate2::read::GzDecoder::new(tarball.as_slice())
        .read_to_end(&mut tar)
        .map_err(|e| format!("{url}: {e}"))?;
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
    Ok(root)
}
