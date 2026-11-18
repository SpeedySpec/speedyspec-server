//! Building `.tar.zst` archives, including hostile ones, for tests.

use speedyspec_server_domain::MANIFEST_FILE;
use tar::{EntryType, Header};

/// One archive entry. Paths are written into the header as given, unchecked, so hostile archives can be built.
pub enum Entry<'a> {
    File(&'a str, &'a [u8]),
    Dir(&'a str),
    Symlink(&'a str, &'a str),
    HardLink(&'a str, &'a str),
    Fifo(&'a str),
    /// A regular file whose name is written raw (`../x`, `/etc/x`).
    RawPath(&'a str, &'a [u8]),
}

/// Builds a tar archive of `entries`, compressed with zstd.
pub fn build(entries: &[Entry]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for entry in entries {
        let (name, kind, link, body): (&str, EntryType, Option<&str>, &[u8]) = match entry {
            Entry::File(name, body) | Entry::RawPath(name, body) => {
                (name, EntryType::Regular, None, body)
            }
            Entry::Dir(name) => (name, EntryType::Directory, None, b""),
            Entry::Symlink(name, target) => (name, EntryType::Symlink, Some(target), b""),
            Entry::HardLink(name, target) => (name, EntryType::Link, Some(target), b""),
            Entry::Fifo(name) => (name, EntryType::Fifo, None, b""),
        };
        let mut header = Header::new_gnu();
        header.set_entry_type(kind);
        header.set_size(body.len() as u64);
        header.set_mode(if kind == EntryType::Directory {
            0o755
        } else {
            0o644
        });
        raw_name(&mut header.as_old_mut().name, name);
        if let Some(link) = link {
            raw_name(&mut header.as_old_mut().linkname, link);
        }
        header.set_cksum();
        builder.append(&header, body).expect("writing to memory");
    }
    let tar = builder.into_inner().expect("writing to memory");

    ruzstd::encoding::compress_to_vec(&tar[..], ruzstd::encoding::CompressionLevel::Fastest)
}

fn raw_name(field: &mut [u8; 100], name: &str) {
    assert!(
        name.len() < field.len(),
        "test archive names must be under 100 bytes: {name}"
    );
    field.fill(0);
    field[..name.len()].copy_from_slice(name.as_bytes());
}

/// A release archive: `speedyspec-release.json` holding `manifest_json`, then `files`.
pub fn release(manifest_json: &str, files: &[(&str, &str)]) -> Vec<u8> {
    let mut entries = vec![Entry::File(MANIFEST_FILE, manifest_json.as_bytes())];
    entries.extend(
        files
            .iter()
            .map(|(name, body)| Entry::File(name, body.as_bytes())),
    );
    build(&entries)
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;

    #[test]
    fn archive_round_trips() {
        let bytes = build(&[
            Entry::Dir("dist"),
            Entry::File("dist/index.html", b"hi"),
            Entry::Symlink("dist/latest", "index.html"),
            Entry::HardLink("dist/copy", "dist/index.html"),
            Entry::Fifo("dist/pipe"),
            Entry::RawPath("../escape", b"x"),
        ]);
        let decoder = ruzstd::decoding::StreamingDecoder::new(&bytes[..]).unwrap();
        let mut archive = tar::Archive::new(decoder);
        let mut seen = Vec::new();
        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            let name = String::from_utf8(entry.path_bytes().to_vec()).unwrap();
            let mut body = Vec::new();
            entry.read_to_end(&mut body).unwrap();
            seen.push((name, entry.header().entry_type(), body));
        }
        let names: Vec<&str> = seen.iter().map(|(n, _, _)| n.as_str()).collect();
        assert_eq!(
            names,
            [
                "dist",
                "dist/index.html",
                "dist/latest",
                "dist/copy",
                "dist/pipe",
                "../escape"
            ]
        );
        assert_eq!(seen[1].2, b"hi");
        assert!(seen[2].1.is_symlink() && seen[3].1.is_hard_link() && seen[4].1.is_fifo());
    }

    #[test]
    fn release_helper_writes_manifest_at_root() {
        let bytes = release(r#"{"app":"admin"}"#, &[("dist/index.html", "x")]);
        let decoder = ruzstd::decoding::StreamingDecoder::new(&bytes[..]).unwrap();
        let mut archive = tar::Archive::new(decoder);
        let names: Vec<String> = archive
            .entries()
            .unwrap()
            .map(|e| String::from_utf8(e.unwrap().path_bytes().to_vec()).unwrap())
            .collect();
        assert_eq!(names, ["speedyspec-release.json", "dist/index.html"]);
    }
}
