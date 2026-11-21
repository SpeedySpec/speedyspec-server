//! Where a group and an entry live: names are hashed, so no group or key can name a path of its own.

use sha2::{Digest, Sha256};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// `group NUL key`, the name an entry file records.
pub(crate) fn name(group: &str, key: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(group.len() + 1 + key.len());
    out.extend(group.as_bytes());
    out.push(0);
    out.extend(key.as_bytes());
    out
}

/// `<dir>/g-<first 32 hex of sha256(group)>`.
pub fn group_dir(dir: &str, group: &str) -> String {
    format!("{dir}/g-{}", &hex(&Sha256::digest(group.as_bytes()))[..32])
}

/// `<group dir>/<first 2 hex of sha256(group NUL key)>/<the other 62 hex>`.
pub fn entry_path(dir: &str, group: &str, key: &str) -> String {
    let digest = hex(&Sha256::digest(name(group, key)));
    format!(
        "{}/{}/{}",
        group_dir(dir, group),
        &digest[..2],
        &digest[2..]
    )
}
