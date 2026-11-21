//! The pinned WordPress release, fetched and unpacked once into the same cache `speedyspec-compat` uses.

use std::io::Read;
use std::path::PathBuf;

use sha2::{Digest, Sha256};

/// The release every comparison is made against.
pub const WORDPRESS: &str = "7.1.5";
const URL: &str = "https://wordpress.org/wordpress-7.1.5.tar.gz";
const SHA256: &str = "053857d980eaee8702c617c685948becb9be2523ba0dcbc98e4dbb2300367c79";
const LIMIT: u64 = 256 * 1024 * 1024;

/// The unpacked release's `wordpress/` root.
pub fn wordpress_release() -> Result<PathBuf, String> {
    let cache = std::env::temp_dir().join("speedyspec-compat");
    let dir = cache.join(format!("wordpress-{WORDPRESS}-{}", &SHA256[..16]));
    let root = dir.join("wordpress");
    if root.join("wp-includes/version.php").exists() {
        return Ok(root);
    }
    std::fs::create_dir_all(&cache).map_err(|e| format!("{}: {e}", cache.display()))?;
    let tarball = ureq::get(URL)
        .call()
        .map_err(|e| format!("{URL}: {e}"))?
        .into_body()
        .with_config()
        .limit(LIMIT)
        .read_to_vec()
        .map_err(|e| format!("{URL}: {e}"))?;
    let digest = format!("{:x}", Sha256::digest(&tarball));
    if digest != SHA256 {
        return Err(format!("{URL}: SHA-256 {digest}, pinned {SHA256}"));
    }
    let staging = tempfile::tempdir_in(&cache).map_err(|e| e.to_string())?;
    let mut tar = Vec::new();
    flate2::read::GzDecoder::new(tarball.as_slice())
        .read_to_end(&mut tar)
        .map_err(|e| format!("{URL}: {e}"))?;
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
