//! Where the owner's socket lives, and whether the platform can bind a path that long.

use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use speedyspec_data::DbError;

/// The owner's socket by default: `<file's dir>/<file stem>.owner/owner.sock`.
pub fn owner_socket_for(file: &Path) -> PathBuf {
    let stem = file
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "redb".to_string());
    file.parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!("{stem}.owner"))
        .join("owner.sock")
}

/// The longest socket path the platform binds: `sun_path` less its NUL (103 bytes on macOS, 107 on Linux).
fn socket_path_limit() -> usize {
    // SAFETY: sockaddr_un is plain data; an all-zero value is valid.
    let addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    addr.sun_path.len() - 1
}

/// `Config` naming the path and the limit when `path` is too long to bind.
pub fn check_socket_path(path: &Path) -> Result<(), DbError> {
    let limit = socket_path_limit();
    let len = path.as_os_str().as_bytes().len();
    if len > limit {
        return Err(DbError::Config(format!(
            "the redb owner socket `{}` is {len} bytes, longer than the {limit} bytes a Unix socket path may be; \
             set the engine's `socket` option to a shorter path",
            path.display()
        )));
    }
    Ok(())
}
