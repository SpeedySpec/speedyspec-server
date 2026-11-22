//! Test only: small, valid media files built in code, so no sample binaries are committed.
//!
//! Each function returns a file's bytes. They are minimal but well formed, enough for FFmpeg's demuxers, the MIDI
//! reader and `getimagesize()` to read: silent audio, single-colour images, a one-note MIDI file.

use std::io::Write;

fn le16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn le32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn be16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_be_bytes());
}
fn be32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_be_bytes());
}

/// A 16-bit PCM WAV file of silence.
pub fn wav_pcm16(seconds: f32, rate: u32, channels: u16) -> Vec<u8> {
    let frames = (seconds * rate as f32) as u32;
    let data = frames * u32::from(channels) * 2;
    let mut out = Vec::with_capacity(44 + data as usize);
    out.extend_from_slice(b"RIFF");
    le32(&mut out, 36 + data);
    out.extend_from_slice(b"WAVEfmt ");
    le32(&mut out, 16);
    le16(&mut out, 1);
    le16(&mut out, channels);
    le32(&mut out, rate);
    le32(&mut out, rate * u32::from(channels) * 2);
    le16(&mut out, channels * 2);
    le16(&mut out, 16);
    out.extend_from_slice(b"data");
    le32(&mut out, data);
    out.resize(44 + data as usize, 0);
    out
}

/// `rate` as the 80-bit IEEE extended float AIFF's COMM chunk uses.
fn extended(rate: u32) -> [u8; 10] {
    let exponent = 31 - rate.leading_zeros();
    let mantissa = u64::from(rate) << (63 - exponent);
    let mut out = [0u8; 10];
    out[..2].copy_from_slice(&((16383 + exponent) as u16).to_be_bytes());
    out[2..].copy_from_slice(&mantissa.to_be_bytes());
    out
}

/// A 16-bit PCM AIFF file of silence.
pub fn aiff_pcm16(seconds: f32, rate: u32, channels: u16) -> Vec<u8> {
    let frames = (seconds * rate as f32) as u32;
    let data = frames * u32::from(channels) * 2;
    let mut out = Vec::new();
    out.extend_from_slice(b"FORM");
    be32(&mut out, 4 + 8 + 18 + 8 + 8 + data);
    out.extend_from_slice(b"AIFFCOMM");
    be32(&mut out, 18);
    be16(&mut out, channels);
    be32(&mut out, frames);
    be16(&mut out, 16);
    out.extend_from_slice(&extended(rate));
    out.extend_from_slice(b"SSND");
    be32(&mut out, 8 + data);
    be32(&mut out, 0);
    be32(&mut out, 0);
    out.resize(out.len() + data as usize, 0);
    out
}

/// Silent MPEG-1 Layer III frames at 128 kb/s, 44.1 kHz, joint stereo: 417 bytes each, 1152 samples each.
pub fn mp3_cbr(frames: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(frames as usize * 417);
    for _ in 0..frames {
        let start = out.len();
        out.extend_from_slice(&[0xFF, 0xFB, 0x90, 0x64]);
        out.resize(start + 417, 0);
    }
    out
}

fn id3v2_frame(out: &mut Vec<u8>, id: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(id);
    be32(out, body.len() as u32);
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(body);
}

/// `mp3_cbr(frames)` behind an ID3v2.3 tag with a title, an artist and, optionally, a PNG front cover.
pub fn mp3_with_id3v2(frames: u32, title: &str, artist: &str, cover_png: Option<&[u8]>) -> Vec<u8> {
    let mut body = Vec::new();
    let text = |s: &str| [&[0u8][..], s.as_bytes()].concat();
    id3v2_frame(&mut body, b"TIT2", &text(title));
    id3v2_frame(&mut body, b"TPE1", &text(artist));
    if let Some(png) = cover_png {
        let mut apic = vec![0u8];
        apic.extend_from_slice(b"image/png\0");
        apic.push(3);
        apic.push(0);
        apic.extend_from_slice(png);
        id3v2_frame(&mut body, b"APIC", &apic);
    }
    let size = body.len() as u32;
    let mut out = b"ID3\x03\x00\x00".to_vec();
    out.extend_from_slice(&[
        ((size >> 21) & 0x7F) as u8,
        ((size >> 14) & 0x7F) as u8,
        ((size >> 7) & 0x7F) as u8,
        (size & 0x7F) as u8,
    ]);
    out.extend_from_slice(&body);
    out.extend_from_slice(&mp3_cbr(frames));
    out
}

/// `mp3_cbr(frames)` followed by an ID3v1 tag holding only a title.
pub fn mp3_with_id3v1(frames: u32, title: &str) -> Vec<u8> {
    let mut out = mp3_cbr(frames);
    let mut tag = vec![0u8; 128];
    tag[..3].copy_from_slice(b"TAG");
    let title = title.as_bytes();
    let n = title.len().min(30);
    tag[3..3 + n].copy_from_slice(&title[..n]);
    tag[127] = 255;
    out.extend_from_slice(&tag);
    out
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in bytes {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    be32(out, data.len() as u32);
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    be32(out, crc);
}

/// An 8-bit RGB PNG of black pixels, its image data in zlib stored (uncompressed) blocks.
pub fn png(width: u32, height: u32) -> Vec<u8> {
    let raw: Vec<u8> = (0..height).flat_map(|_| std::iter::once(0u8).chain(vec![0u8; width as usize * 3])).collect();
    let mut zlib = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = if raw.is_empty() { vec![&[][..]] } else { raw.chunks(65535).collect() };
    for (i, block) in blocks.iter().enumerate() {
        zlib.push(u8::from(i + 1 == blocks.len()));
        le16(&mut zlib, block.len() as u16);
        le16(&mut zlib, !(block.len() as u16));
        zlib.extend_from_slice(block);
    }
    be32(&mut zlib, adler32(&raw));
    let mut ihdr = Vec::new();
    be32(&mut ihdr, width);
    be32(&mut ihdr, height);
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    png_chunk(&mut out, b"IHDR", &ihdr);
    png_chunk(&mut out, b"IDAT", &zlib);
    png_chunk(&mut out, b"IEND", &[]);
    out
}

/// A two-colour GIF89a of colour 0. Its LZW data resets the table before every pixel, so codes stay 3 bits wide.
pub fn gif(width: u16, height: u16) -> Vec<u8> {
    let mut out = b"GIF89a".to_vec();
    le16(&mut out, width);
    le16(&mut out, height);
    out.extend_from_slice(&[0x80, 0, 0, 0, 0, 0, 0xFF, 0xFF, 0xFF]);
    out.push(0x2C);
    le16(&mut out, 0);
    le16(&mut out, 0);
    le16(&mut out, width);
    le16(&mut out, height);
    out.push(0);
    out.push(2);
    let (clear, end) = (4u32, 5u32);
    let mut codes = Vec::new();
    for _ in 0..u32::from(width) * u32::from(height) {
        codes.push(clear);
        codes.push(0);
    }
    codes.push(end);
    let mut packed = Vec::new();
    let (mut acc, mut bits) = (0u32, 0u32);
    for code in codes {
        acc |= code << bits;
        bits += 3;
        while bits >= 8 {
            packed.push(acc as u8);
            acc >>= 8;
            bits -= 8;
        }
    }
    if bits > 0 {
        packed.push(acc as u8);
    }
    for block in packed.chunks(255) {
        out.push(block.len() as u8);
        out.extend_from_slice(block);
    }
    out.push(0);
    out.push(0x3B);
    out
}

/// A 24-bit BMP of black pixels.
pub fn bmp(width: u32, height: u32) -> Vec<u8> {
    let row = (width * 3).div_ceil(4) * 4;
    let data = row * height;
    let mut out = b"BM".to_vec();
    le32(&mut out, 54 + data);
    le32(&mut out, 0);
    le32(&mut out, 54);
    le32(&mut out, 40);
    le32(&mut out, width);
    le32(&mut out, height);
    le16(&mut out, 1);
    le16(&mut out, 24);
    le32(&mut out, 0);
    le32(&mut out, data);
    le32(&mut out, 2835);
    le32(&mut out, 2835);
    le32(&mut out, 0);
    le32(&mut out, 0);
    out.resize(out.len() + data as usize, 0);
    out
}

/// `value` as a MIDI variable-length quantity.
pub fn vlq(value: u32) -> Vec<u8> {
    let mut bytes = vec![(value & 0x7F) as u8];
    let mut rest = value >> 7;
    while rest > 0 {
        bytes.insert(0, (rest & 0x7F) as u8 | 0x80);
        rest >>= 7;
    }
    bytes
}

/// A format 0 MIDI file: a track name, one tempo, and one note held for `length_ticks`.
pub fn midi(tempo_us: u32, ticks: u16, length_ticks: u32, track_name: &str) -> Vec<u8> {
    let mut track = vec![0x00, 0xFF, 0x03];
    track.extend_from_slice(&vlq(track_name.len() as u32));
    track.extend_from_slice(track_name.as_bytes());
    track.extend_from_slice(&[0x00, 0xFF, 0x51, 0x03]);
    track.extend_from_slice(&tempo_us.to_be_bytes()[1..]);
    track.extend_from_slice(&[0x00, 0x90, 0x3C, 0x40]);
    track.extend_from_slice(&vlq(length_ticks));
    track.extend_from_slice(&[0x80, 0x3C, 0x40, 0x00, 0xFF, 0x2F, 0x00]);
    let mut out = b"MThd".to_vec();
    be32(&mut out, 6);
    be16(&mut out, 0);
    be16(&mut out, 1);
    be16(&mut out, ticks);
    out.extend_from_slice(b"MTrk");
    be32(&mut out, track.len() as u32);
    out.extend_from_slice(&track);
    out
}

/// `bytes` in a temporary file ending in `.ext`, removed when dropped.
pub fn write_temp(bytes: &[u8], ext: &str) -> tempfile::NamedTempFile {
    let mut file = tempfile::Builder::new().suffix(&format!(".{ext}")).tempfile().expect("a temporary file");
    file.write_all(bytes).expect("writing a fixture");
    file.flush().expect("flushing a fixture");
    file
}

/// Every fixture with a file name, for fuzz seeds and benchmarks.
pub fn all() -> Vec<(&'static str, Vec<u8>)> {
    let png = png(3, 2);
    vec![
        ("silence.wav", wav_pcm16(1.0, 8000, 1)),
        ("silence.aiff", aiff_pcm16(1.0, 8000, 1)),
        ("silence.mp3", mp3_cbr(40)),
        ("tagged.mp3", mp3_with_id3v2(40, "Title", "Artist", Some(&png))),
        ("v1.mp3", mp3_with_id3v1(40, "Old")),
        ("black.png", png),
        ("black.gif", gif(4, 3)),
        ("black.bmp", bmp(4, 3)),
        ("note.mid", midi(500_000, 480, 1920, "Song")),
    ]
}
