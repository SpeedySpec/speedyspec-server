//! Reading a static release into memory: content types, ETags, compressed variants and cache rules.

use std::collections::HashMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use bytes::Bytes;
use globset::{GlobBuilder, GlobMatcher};
use speedyspec_server_domain::StaticSection;

/// The default cap on one release's file set in memory: 512 MiB.
pub const DEFAULT_LIMIT_BYTES: u64 = 512 * 1024 * 1024;

/// Files at or below this size are never compressed.
const COMPRESS_MIN_BYTES: usize = 1024;

/// One file, ready to serve.
#[derive(Clone, Debug)]
pub struct StaticFile {
    pub content_type: String,
    /// A strong ETag, quotes included.
    pub etag: String,
    pub cache_control: String,
    pub identity: Bytes,
    pub br: Option<Bytes>,
    pub gzip: Option<Bytes>,
}

/// Every file of one static release, keyed by its `/`-separated path relative to the release's root. Immutable once
/// built.
#[derive(Clone, Debug)]
pub struct StaticSet {
    pub(crate) files: HashMap<String, StaticFile>,
    pub(crate) not_found: Option<String>,
    /// The release's maintenance page, when it names one.
    maintenance: Option<Bytes>,
    total_bytes: u64,
}

/// Why a static release could not be loaded.
#[derive(Debug, thiserror::Error)]
pub enum StaticBuildError {
    #[error("static root {0} does not exist or is not a directory")]
    RootMissing(PathBuf),
    #[error("static.not_found file {0:?} is not in the release")]
    NotFoundMissing(String),
    #[error("static.maintenance_page file {0:?} is not in the release")]
    MaintenancePageMissing(String),
    #[error("static files need {needed} bytes in memory, over the limit of {limit}")]
    LimitExceeded { limit: u64, needed: u64 },
    #[error("cache rule pattern {0:?} is not a valid glob")]
    BadPattern(String),
    #[error("reading {0}: {1}")]
    Io(PathBuf, io::Error),
    #[error("{0} is not a regular file or directory with a UTF-8 name")]
    UnsupportedFile(PathBuf),
}

impl StaticSet {
    /// Reads every file under `release_dir/<section.root>` into memory, refusing to need more than `limit_bytes`.
    pub fn build(
        release_dir: &Path,
        section: &StaticSection,
        limit_bytes: u64,
    ) -> Result<StaticSet, StaticBuildError> {
        let root = release_dir.join(&section.root);
        if !root.is_dir() {
            return Err(StaticBuildError::RootMissing(root));
        }
        let rules = section
            .cache
            .iter()
            .map(|rule| {
                let glob = GlobBuilder::new(&rule.pattern)
                    .literal_separator(true)
                    .build()
                    .map_err(|_| StaticBuildError::BadPattern(rule.pattern.clone()))?;
                Ok((glob.compile_matcher(), rule.cache_control.clone()))
            })
            .collect::<Result<Vec<(GlobMatcher, String)>, StaticBuildError>>()?;

        let mut paths = Vec::new();
        collect(&root, "", &mut paths)?;

        let mut files = HashMap::with_capacity(paths.len());
        let mut total_bytes = 0u64;
        for (rel, path) in paths {
            // Check the size on disk first, so a file far over the limit is never read into memory.
            let on_disk = std::fs::metadata(&path)
                .map_err(|err| StaticBuildError::Io(path.clone(), err))?
                .len();
            if total_bytes + on_disk > limit_bytes {
                return Err(StaticBuildError::LimitExceeded {
                    limit: limit_bytes,
                    needed: total_bytes + on_disk,
                });
            }
            let file = load(&path, &rel, &rules)?;
            total_bytes += size(&file);
            if total_bytes > limit_bytes {
                return Err(StaticBuildError::LimitExceeded {
                    limit: limit_bytes,
                    needed: total_bytes,
                });
            }
            files.insert(rel, file);
        }
        if let Some(name) = &section.not_found
            && !files.contains_key(name)
        {
            return Err(StaticBuildError::NotFoundMissing(name.clone()));
        }

        let maintenance = match &section.maintenance_page {
            None => None,
            Some(name) => Some(
                files
                    .get(name)
                    .map(|file| file.identity.clone())
                    .ok_or_else(|| StaticBuildError::MaintenancePageMissing(name.clone()))?,
            ),
        };

        Ok(StaticSet {
            files,
            not_found: section.not_found.clone(),
            maintenance,
            total_bytes,
        })
    }

    /// Only the files whose `Cache-Control` says `immutable` (hashed assets), and no not-found page: what is kept of
    /// an old release for the grace period after a switch.
    pub fn immutable_only(&self) -> StaticSet {
        let files: HashMap<String, StaticFile> = self
            .files
            .iter()
            .filter(|(_, file)| file.cache_control.contains("immutable"))
            .map(|(rel, file)| (rel.clone(), file.clone()))
            .collect();
        let total_bytes = files.values().map(size).sum();
        StaticSet {
            files,
            not_found: None,
            maintenance: None,
            total_bytes,
        }
    }

    /// The release's maintenance page, held in memory.
    pub fn maintenance_page(&self) -> Option<&Bytes> {
        self.maintenance.as_ref()
    }

    /// Bytes held: every identity body and every kept compressed variant.
    pub fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    /// The file at a `/`-separated path relative to the root, without a leading slash.
    pub fn file(&self, rel: &str) -> Option<&StaticFile> {
        self.files.get(rel)
    }
}

fn size(file: &StaticFile) -> u64 {
    let variants =
        file.br.as_ref().map_or(0, Bytes::len) + file.gzip.as_ref().map_or(0, Bytes::len);
    (file.identity.len() + variants) as u64
}

/// Walks `dir` without following symlinks, collecting `(relative path, full path)` for every regular file.
fn collect(
    dir: &Path,
    prefix: &str,
    out: &mut Vec<(String, PathBuf)>,
) -> Result<(), StaticBuildError> {
    let entries =
        std::fs::read_dir(dir).map_err(|err| StaticBuildError::Io(dir.to_path_buf(), err))?;
    for entry in entries {
        let entry = entry.map_err(|err| StaticBuildError::Io(dir.to_path_buf(), err))?;
        let path = entry.path();
        let kind = entry
            .file_type()
            .map_err(|err| StaticBuildError::Io(path.clone(), err))?;
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            return Err(StaticBuildError::UnsupportedFile(path));
        };
        let rel = format!("{prefix}{name}");
        if kind.is_dir() {
            collect(&path, &format!("{rel}/"), out)?;
        } else if kind.is_file() {
            out.push((rel, path));
        } else {
            return Err(StaticBuildError::UnsupportedFile(path));
        }
    }

    Ok(())
}

fn load(
    path: &Path,
    rel: &str,
    rules: &[(GlobMatcher, String)],
) -> Result<StaticFile, StaticBuildError> {
    let identity =
        std::fs::read(path).map_err(|err| StaticBuildError::Io(path.to_path_buf(), err))?;
    let mime = mime_guess::from_path(rel).first_or_octet_stream();
    let essence = mime.essence_str().to_string();
    let content_type = if has_charset(&essence) {
        format!("{essence}; charset=utf-8")
    } else {
        essence.clone()
    };
    let hash = blake3::hash(&identity);
    let etag = format!("\"{}\"", hex(&hash.as_bytes()[..16]));
    let cache_control = rules
        .iter()
        .find(|(glob, _)| glob.is_match(rel))
        .map_or_else(|| "no-cache".to_string(), |(_, value)| value.clone());

    let (br, gzip) = if compressible(&essence) && identity.len() > COMPRESS_MIN_BYTES {
        (
            smaller(brotli(&identity), identity.len()),
            smaller(gzip(&identity), identity.len()),
        )
    } else {
        (None, None)
    };

    Ok(StaticFile {
        content_type,
        etag,
        cache_control,
        identity: Bytes::from(identity),
        br,
        gzip,
    })
}

fn has_charset(essence: &str) -> bool {
    essence.starts_with("text/")
        || matches!(
            essence,
            "application/javascript" | "application/json" | "image/svg+xml"
        )
}

fn compressible(essence: &str) -> bool {
    essence.starts_with("text/")
        || matches!(
            essence,
            "application/javascript"
                | "application/json"
                | "application/manifest+json"
                | "application/xml"
                | "image/svg+xml"
                | "application/wasm"
        )
}

fn smaller(compressed: Vec<u8>, original: usize) -> Option<Bytes> {
    (compressed.len() < original).then(|| Bytes::from(compressed))
}

fn brotli(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut writer = brotli::CompressorWriter::new(&mut out, 4096, 11, 22);
        writer.write_all(input).expect("writing to memory");
    }
    out
}

fn gzip(input: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(9));
    encoder.write_all(input).expect("writing to memory");
    encoder.finish().expect("writing to memory")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
