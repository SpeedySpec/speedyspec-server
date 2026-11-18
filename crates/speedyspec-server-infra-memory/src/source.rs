//! A release source serving indexes and files from memory.

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

use speedyspec_server_domain::{ReleaseIndex, ReleaseSource, SourceError};

/// Serves indexes and files registered by URL. Downloads resume from the destination's length, like the HTTP source.
#[derive(Default)]
pub struct MemorySource {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    indexes: HashMap<String, String>,
    files: HashMap<String, Vec<u8>>,
    fail_after: HashMap<String, usize>,
    downloads: Vec<String>,
    delay: std::time::Duration,
}

fn error(url: &str, message: &str) -> SourceError {
    SourceError {
        url: url.to_string(),
        message: message.to_string(),
    }
}

impl MemorySource {
    pub fn new() -> MemorySource {
        MemorySource::default()
    }

    pub fn add_index(&self, url: &str, json: &str) {
        self.inner
            .lock()
            .unwrap()
            .indexes
            .insert(url.to_string(), json.to_string());
    }

    pub fn add_file(&self, url: &str, bytes: Vec<u8>) {
        self.inner
            .lock()
            .unwrap()
            .files
            .insert(url.to_string(), bytes);
    }

    /// The next download of `url` writes until the file holds `after_bytes` bytes, then fails.
    pub fn fail_next_download(&self, url: &str, after_bytes: usize) {
        self.inner
            .lock()
            .unwrap()
            .fail_after
            .insert(url.to_string(), after_bytes);
    }

    /// Every download sleeps this long first, without holding any lock.
    pub fn set_download_delay(&self, delay: std::time::Duration) {
        self.inner.lock().unwrap().delay = delay;
    }

    /// Every URL downloaded so far, in order.
    pub fn downloads(&self) -> Vec<String> {
        self.inner.lock().unwrap().downloads.clone()
    }
}

impl ReleaseSource for MemorySource {
    fn index(&self, url: &str) -> Result<ReleaseIndex, SourceError> {
        let inner = self.inner.lock().unwrap();
        let json = inner
            .indexes
            .get(url)
            .ok_or_else(|| error(url, "404 not found"))?;
        ReleaseIndex::parse(json.as_bytes()).map_err(|err| error(url, &err.to_string()))
    }

    fn download(&self, url: &str, dest: &Path) -> Result<(), SourceError> {
        let delay = self.inner.lock().unwrap().delay;
        std::thread::sleep(delay);
        let mut inner = self.inner.lock().unwrap();
        inner.downloads.push(url.to_string());
        let bytes = inner
            .files
            .get(url)
            .cloned()
            .ok_or_else(|| error(url, "404 not found"))?;
        let have = std::fs::metadata(dest)
            .map(|m| m.len() as usize)
            .unwrap_or(0)
            .min(bytes.len());
        let fail_at = inner.fail_after.remove(url);
        let end = fail_at.map_or(bytes.len(), |at| at.min(bytes.len()));
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dest)
            .map_err(|err| error(url, &err.to_string()))?;
        if end > have {
            file.write_all(&bytes[have..end])
                .map_err(|err| error(url, &err.to_string()))?;
        }
        if fail_at.is_some() {
            return Err(error(url, "connection reset"));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_source_resumes_like_http() {
        let source = MemorySource::new();
        source.add_file("https://x/a", b"0123456789".to_vec());
        let dir = std::env::temp_dir().join(format!("memsrc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("a");
        std::fs::write(&dest, b"0123").unwrap();
        source.download("https://x/a", &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"0123456789");

        std::fs::remove_file(&dest).unwrap();
        source.fail_next_download("https://x/a", 3);
        assert!(source.download("https://x/a", &dest).is_err());
        assert_eq!(std::fs::read(&dest).unwrap(), b"012");
        source.download("https://x/a", &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"0123456789");
        assert_eq!(
            source.downloads(),
            ["https://x/a", "https://x/a", "https://x/a"]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn unknown_url_is_an_error() {
        let source = MemorySource::new();
        assert!(source.index("https://x/none").is_err());
    }
}
