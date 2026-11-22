//! A bounded Standard MIDI File reader (FFmpeg has no MIDI demuxer).
//!
//! It reads only what WordPress needs from a `.mid` file: its duration, computed from the tempo map, and the first
//! track's name, copyright and text events as tags. Every event is skipped by its declared length (running status,
//! SysEx and meta events included), so the parse is linear and bounded by the file.

use std::fs::File;
use std::io::Read;
use std::time::Duration;

use speedyspec_wp_media_contracts::{
    Codec, ContainerKind, Limit, MediaInfo, MediaReader, MediaSource, ProbeLimits, RawViews,
    ReadError, Stream, Tag, TagKey, TagSystem, Tree,
};

/// The default tempo when a file sets none: 120 quarter notes per minute.
const DEFAULT_TEMPO_US: u64 = 500_000;

/// Reads Standard MIDI Files (Adapter behind the media reader port).
pub struct MidiReader;

impl MediaReader for MidiReader {
    fn read(&self, source: MediaSource, limits: &ProbeLimits) -> Result<MediaInfo, ReadError> {
        let file = match source {
            MediaSource::Path(path) => File::open(&path).map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => ReadError::NotFound,
                _ => ReadError::Io(e.to_string()),
            })?,
            MediaSource::Fd(fd) => File::from(fd),
        };
        if !file
            .metadata()
            .map_err(|e| ReadError::Io(e.to_string()))?
            .is_file()
        {
            return Err(ReadError::Io("not a regular file".into()));
        }
        let mut bytes = Vec::new();
        file.take(limits.max_bytes.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|e| ReadError::Io(e.to_string()))?;
        if bytes.len() as u64 > limits.max_bytes {
            return Err(ReadError::LimitExceeded(Limit::Bytes));
        }
        let mut info = parse(&bytes, limits)?;
        info.file_size = Some(bytes.len() as u64);
        Ok(info)
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], ReadError> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&e| e <= self.bytes.len())
            .ok_or(ReadError::Truncated)?;
        let out = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(out)
    }
    fn byte(&mut self) -> Result<u8, ReadError> {
        Ok(self.take(1)?[0])
    }
    fn be32(&mut self) -> Result<u32, ReadError> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    /// A variable-length quantity: at most 4 bytes.
    fn vlq(&mut self) -> Result<u32, ReadError> {
        let mut value = 0u32;
        for _ in 0..4 {
            let b = self.byte()?;
            value = (value << 7) | u32::from(b & 0x7F);
            if b & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(ReadError::Truncated)
    }
    fn done(&self) -> bool {
        self.pos >= self.bytes.len()
    }
}

#[derive(Default)]
struct Track {
    end_tick: u64,
    tempos: Vec<(u64, u64)>,
    texts: Vec<(u8, String)>,
}

fn parse_track(data: &[u8]) -> Result<Track, ReadError> {
    let mut c = Cursor {
        bytes: data,
        pos: 0,
    };
    let mut track = Track::default();
    let mut tick = 0u64;
    let mut running: Option<u8> = None;
    while !c.done() {
        tick += u64::from(c.vlq()?);
        let first = c.byte()?;
        let status = if first & 0x80 != 0 {
            first
        } else {
            c.pos -= 1;
            running.ok_or(ReadError::Truncated)?
        };
        match status {
            0xFF => {
                let kind = c.byte()?;
                let len = c.vlq()? as usize;
                let body = c.take(len)?;
                match kind {
                    0x51 if len == 3 => {
                        let us =
                            u64::from(body[0]) << 16 | u64::from(body[1]) << 8 | u64::from(body[2]);
                        track.tempos.push((tick, us));
                    }
                    0x01..=0x03 => track
                        .texts
                        .push((kind, String::from_utf8_lossy(body).into_owned())),
                    0x2F => {
                        track.end_tick = tick;
                        return Ok(track);
                    }
                    _ => {}
                }
                running = None;
            }
            0xF0 | 0xF7 => {
                let len = c.vlq()? as usize;
                c.take(len)?;
                running = None;
            }
            0x80..=0xEF => {
                let data_bytes = if matches!(status & 0xF0, 0xC0 | 0xD0) {
                    1
                } else {
                    2
                };
                c.take(data_bytes)?;
                running = Some(status);
            }
            _ => return Err(ReadError::Truncated),
        }
    }
    track.end_tick = tick;
    Ok(track)
}

/// Seconds at `tick` under a tempo map of `(tick, µs per quarter note)` sorted by tick.
fn seconds_at(tick: u64, tempos: &[(u64, u64)], per_quarter: u64) -> f64 {
    let mut seconds = 0.0;
    let mut last_tick = 0u64;
    let mut tempo = DEFAULT_TEMPO_US;
    for &(at, us) in tempos {
        if at >= tick {
            break;
        }
        seconds += (at - last_tick) as f64 * tempo as f64 / per_quarter as f64 / 1e6;
        last_tick = at;
        tempo = us;
    }
    seconds + (tick - last_tick) as f64 * tempo as f64 / per_quarter as f64 / 1e6
}

/// Describes the MIDI file in `bytes`.
pub fn parse(bytes: &[u8], limits: &ProbeLimits) -> Result<MediaInfo, ReadError> {
    if bytes.len() as u64 > limits.max_bytes {
        return Err(ReadError::LimitExceeded(Limit::Bytes));
    }
    if !bytes.starts_with(b"MThd") {
        return Err(ReadError::Unrecognized);
    }
    let mut c = Cursor { bytes, pos: 4 };
    let header_len = c.be32()? as usize;
    if header_len < 6 {
        return Err(ReadError::Truncated);
    }
    let header = c.take(header_len)?;
    let format = u16::from_be_bytes([header[0], header[1]]);
    let declared = u16::from_be_bytes([header[2], header[3]]);
    let division = u16::from_be_bytes([header[4], header[5]]);

    let mut tracks = Vec::new();
    while !c.done() && tracks.len() < usize::from(declared) {
        let kind = c.take(4)?;
        let len = c.be32()? as usize;
        let data = c.take(len)?;
        if kind == b"MTrk" {
            tracks.push(parse_track(data)?);
        }
    }
    if tracks.len() < usize::from(declared) {
        return Err(ReadError::Truncated);
    }

    let seconds = if division & 0x8000 != 0 {
        let fps = match (division >> 8) as u8 as i8 {
            -24 => 24.0,
            -25 => 25.0,
            -29 => 29.97,
            -30 => 30.0,
            _ => return Err(ReadError::Truncated),
        };
        let per_frame = f64::from(division & 0xFF);
        if per_frame == 0.0 {
            return Err(ReadError::Truncated);
        }
        tracks.iter().map(|t| t.end_tick).max().unwrap_or(0) as f64 / (fps * per_frame)
    } else {
        let per_quarter = u64::from(division.max(1));
        if format == 2 {
            tracks
                .iter()
                .map(|t| {
                    let mut tempos = t.tempos.clone();
                    tempos.sort_by_key(|&(at, _)| at);
                    seconds_at(t.end_tick, &tempos, per_quarter)
                })
                .fold(0.0, f64::max)
        } else {
            let mut tempos: Vec<(u64, u64)> = tracks
                .iter()
                .flat_map(|t| t.tempos.iter().copied())
                .collect();
            tempos.sort_by_key(|&(at, _)| at);
            let end = tracks.iter().map(|t| t.end_tick).max().unwrap_or(0);
            seconds_at(end, &tempos, per_quarter)
        }
    };

    let mut tags = Vec::new();
    if let Some(first) = tracks.first() {
        for (kind, key) in [
            (0x03u8, TagKey::Title),
            (0x02, TagKey::Copyright),
            (0x01, TagKey::Comment),
        ] {
            if let Some((_, text)) = first.texts.iter().find(|(k, _)| *k == kind) {
                tags.push(Tag {
                    key,
                    system: TagSystem::Midi,
                    values: vec![text.clone()],
                });
            }
        }
    }
    let tempo_changes: usize = tracks.iter().map(|t| t.tempos.len()).sum();
    let raw = Tree::Map(vec![
        ("format".into(), Tree::Int(i64::from(format))),
        ("tracks".into(), Tree::Int(tracks.len() as i64)),
        ("division".into(), Tree::Int(i64::from(division))),
        ("tempo_changes".into(), Tree::Int(tempo_changes as i64)),
    ]);

    Ok(MediaInfo {
        container: ContainerKind::Midi,
        mime_type: Some("audio/midi".into()),
        duration: Some(Duration::from_secs_f64(seconds.max(0.0))),
        bitrate: None,
        file_size: Some(bytes.len() as u64),
        created_at: None,
        streams: vec![Stream::Data {
            codec: Codec::Other("midi".into()),
        }],
        tags,
        pictures: Vec::new(),
        raw: RawViews {
            getid3: None,
            reader: Some(("midi".into(), raw)),
        },
    })
}
