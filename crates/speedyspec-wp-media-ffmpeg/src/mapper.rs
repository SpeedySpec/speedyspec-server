//! The Data Mapper from FFmpeg's description (`RawProbe`) to `MediaInfo`.

use std::time::Duration;

use speedyspec_wp_media_contracts::{Codec, ContainerKind, MediaInfo, RawViews, Stream};

use crate::RawProbe;

/// `probe` as a library-neutral `MediaInfo`.
pub fn map(probe: RawProbe) -> MediaInfo {
    MediaInfo {
        container: ContainerKind::Other(probe.format_name.clone()),
        mime_type: None,
        duration: probe.duration_us.map(|us| Duration::from_micros(us as u64)),
        bitrate: probe.bit_rate.map(|b| b as u64),
        file_size: Some(probe.file_size),
        created_at: None,
        streams: probe
            .streams
            .iter()
            .map(|s| Stream::Data { codec: Codec::Other(s.codec_name.clone()) })
            .collect(),
        tags: Vec::new(),
        pictures: Vec::new(),
        raw: RawViews::default(),
    }
}
