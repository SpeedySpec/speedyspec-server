//! Unpacking a `.tar.zst` release, refusing anything that could write outside it.

use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};

use tar::EntryType;

/// The most file content one release may unpack to: 4 GiB.
pub const MAX_UNPACKED_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// Unpacks `archive` into `dest`, which must exist and be empty. Refuses absolute paths, `..`, symlinks that are
/// absolute or resolve outside `dest`, entries reached through a symlink, hard links, devices and FIFOs, and more
/// than `limit` bytes of content. The error names the entry.
pub(crate) fn unpack(archive: &Path, dest: &Path, limit: u64) -> Result<(), String> {
    let file =
        File::open(archive).map_err(|err| format!("opening {}: {err}", archive.display()))?;
    let decoder = ruzstd::decoding::StreamingDecoder::new(file)
        .map_err(|err| format!("not a zstd archive: {err}"))?;
    let mut tar = tar::Archive::new(decoder);
    let mut total = 0u64;
    let entries = tar
        .entries()
        .map_err(|err| format!("not a tar archive: {err}"))?;
    for entry in entries {
        let mut entry = entry.map_err(|err| format!("reading the archive: {err}"))?;
        let name = String::from_utf8(entry.path_bytes().to_vec())
            .map_err(|_| "an entry name is not UTF-8".to_string())?;
        let fail = |why: &str| format!("entry {name}: {why}");
        let rel = relative(&name).ok_or_else(|| fail("absolute paths and `..` are not allowed"))?;
        if rel.as_os_str().is_empty() {
            continue;
        }
        let target = dest.join(&rel);
        if through_symlink(dest, &rel) {
            return Err(fail("is reached through a symlink"));
        }

        match entry.header().entry_type() {
            EntryType::Directory => {
                std::fs::create_dir_all(&target).map_err(|err| fail(&err.to_string()))?
            }
            EntryType::Regular | EntryType::Continuous => {
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent).map_err(|err| fail(&err.to_string()))?;
                }
                let _ = std::fs::remove_file(&target);
                let mut out = File::create(&target).map_err(|err| fail(&err.to_string()))?;
                total += copy_limited(&mut entry, &mut out, limit.saturating_sub(total))
                    .map_err(|err| fail(&err.to_string()))?;
                if total > limit {
                    return Err(fail(&format!(
                        "the release unpacks to more than {limit} bytes"
                    )));
                }
            }
            EntryType::Symlink => {
                let link = entry
                    .link_name_bytes()
                    .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                    .ok_or_else(|| fail("symlink without a target"))?;
                if !stays_inside(&rel, &link) {
                    return Err(fail(&format!(
                        "symlink to {link} points outside the release"
                    )));
                }
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent).map_err(|err| fail(&err.to_string()))?;
                }
                std::os::unix::fs::symlink(&link, &target).map_err(|err| fail(&err.to_string()))?;
            }
            EntryType::Link => return Err(fail("hard links are not allowed")),
            // Metadata for the whole archive (`git archive` writes one first); nothing to unpack.
            EntryType::XGlobalHeader => {}
            other => return Err(fail(&format!("entries of type {other:?} are not allowed"))),
        }
    }

    // A symlink can only be judged once every symlink it may pass through exists, in any order in the archive.
    check_symlinks(dest, Path::new(""))?;

    Ok(())
}

/// Most symlinks followed while resolving one target.
const MAX_HOPS: usize = 40;

/// Refuses any symlink under `root/rel` that resolves, following other symlinks inside the release, outside it.
fn check_symlinks(root: &Path, rel: &Path) -> Result<(), String> {
    let dir = root.join(rel);
    let entries =
        std::fs::read_dir(&dir).map_err(|err| format!("reading {}: {err}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|err| format!("reading {}: {err}", dir.display()))?;
        let child = rel.join(entry.file_name());
        let kind = entry
            .file_type()
            .map_err(|err| format!("reading {}: {err}", child.display()))?;
        if kind.is_symlink() {
            if !resolves_inside(root, &child) {
                return Err(format!(
                    "entry {}: symlink resolves outside the release",
                    child.display()
                ));
            }
        } else if kind.is_dir() {
            check_symlinks(root, &child)?;
        }
    }
    Ok(())
}

/// Resolves the symlink at `root/link`, component by component, following symlinks inside `root`; `false` as soon
/// as the path leaves `root`, is absolute, or loops.
fn resolves_inside(root: &Path, link: &Path) -> bool {
    use std::collections::VecDeque;

    let mut stack: Vec<std::ffi::OsString> = link
        .parent()
        .map(|p| p.iter().map(ToOwned::to_owned).collect())
        .unwrap_or_default();
    let Ok(target) = std::fs::read_link(root.join(link)) else {
        return false;
    };
    let mut pending: VecDeque<std::ffi::OsString> = VecDeque::new();
    if target.is_absolute() {
        return false;
    }
    pending.extend(target.iter().map(ToOwned::to_owned));
    let mut hops = 0;
    while let Some(part) = pending.pop_front() {
        match part.to_str() {
            Some(".") => {}
            Some("..") => {
                if stack.pop().is_none() {
                    return false;
                }
            }
            _ => {
                stack.push(part);
                let here: PathBuf = root.join(stack.iter().collect::<PathBuf>());
                if std::fs::symlink_metadata(&here).is_ok_and(|meta| meta.file_type().is_symlink())
                {
                    hops += 1;
                    let Ok(next) = std::fs::read_link(&here) else {
                        return false;
                    };
                    if hops > MAX_HOPS || next.is_absolute() {
                        return false;
                    }
                    stack.pop();
                    for component in next.iter().rev() {
                        pending.push_front(component.to_owned());
                    }
                }
            }
        }
    }
    true
}

/// The entry's path relative to the release, or `None` if it is absolute or uses `..`.
fn relative(name: &str) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for component in Path::new(name).components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(out)
}

/// Whether any directory above `rel` inside `root` is a symlink. Writing through one could land anywhere it points.
fn through_symlink(root: &Path, rel: &Path) -> bool {
    let mut current = root.to_path_buf();
    let parts: Vec<_> = rel.components().collect();
    for part in &parts[..parts.len().saturating_sub(1)] {
        current.push(part);
        if std::fs::symlink_metadata(&current).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return true;
        }
    }
    false
}

/// Whether a symlink at `rel` with target `link` resolves, lexically, inside the release.
fn stays_inside(rel: &Path, link: &str) -> bool {
    if link.starts_with('/') {
        return false;
    }
    let mut depth: Vec<&std::ffi::OsStr> =
        rel.parent().map(|p| p.iter().collect()).unwrap_or_default();
    for component in Path::new(link).components() {
        match component {
            Component::Normal(part) => depth.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                if depth.pop().is_none() {
                    return false;
                }
            }
            Component::RootDir | Component::Prefix(_) => return false,
        }
    }
    true
}

/// Copies at most `budget + 1` bytes, so going over the limit is noticed without reading the rest.
fn copy_limited(from: &mut impl Read, to: &mut impl Write, budget: u64) -> io::Result<u64> {
    io::copy(&mut from.take(budget.saturating_add(1)), to)
}
