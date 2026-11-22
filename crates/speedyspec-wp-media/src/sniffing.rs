//! Chooses a reader by a file's first bytes (Strategy, Composite).

use std::fs::File;
use std::io::Read;
use std::os::unix::fs::FileExt;

use speedyspec_wp_media_contracts::{MediaInfo, MediaReader, MediaSource, ProbeLimits, ReadError};

/// Sends Standard MIDI Files (`MThd`) to the MIDI reader, which FFmpeg can't read, and everything else to the
/// general reader.
pub struct SniffingReader {
    midi: Box<dyn MediaReader>,
    general: Box<dyn MediaReader>,
}

impl SniffingReader {
    pub fn new(midi: Box<dyn MediaReader>, general: Box<dyn MediaReader>) -> SniffingReader {
        SniffingReader { midi, general }
    }
}

/// The first four bytes, or fewer; a descriptor's offset is left where it was (`pread`).
fn magic(source: &MediaSource) -> Vec<u8> {
    let mut head = [0u8; 4];
    let n = match source {
        MediaSource::Path(path) => File::open(path).and_then(|mut f| f.read(&mut head)).unwrap_or(0),
        MediaSource::Fd(fd) => match fd.try_clone() {
            Ok(dup) => File::from(dup).read_at(&mut head, 0).unwrap_or(0),
            Err(_) => 0,
        },
    };
    head[..n].to_vec()
}

impl MediaReader for SniffingReader {
    fn read(&self, source: MediaSource, limits: &ProbeLimits) -> Result<MediaInfo, ReadError> {
        if magic(&source) == b"MThd" {
            self.midi.read(source, limits)
        } else {
            self.general.read(source, limits)
        }
    }
}
