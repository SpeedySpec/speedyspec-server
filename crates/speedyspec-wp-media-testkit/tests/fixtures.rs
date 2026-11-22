use speedyspec_wp_media_testkit as kit;

fn be32(bytes: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap())
}

#[test]
fn wav_header_is_riff_wave() {
    let wav = kit::wav_pcm16(0.5, 8000, 2);
    assert_eq!(&wav[0..4], b"RIFF");
    assert_eq!(&wav[8..12], b"WAVE");
    let data = (0.5 * 8000.0) as usize * 2 * 2;
    assert_eq!(wav.len(), 44 + data);
    assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()) as usize, data);
}

#[test]
fn aiff_header_is_form_aiff() {
    let aiff = kit::aiff_pcm16(0.25, 8000, 1);
    assert_eq!(&aiff[0..4], b"FORM");
    assert_eq!(&aiff[8..12], b"AIFF");
}

#[test]
fn mp3_frames_are_417_bytes() {
    let mp3 = kit::mp3_cbr(10);
    assert_eq!(mp3.len(), 4170);
    for frame in mp3.chunks(417) {
        assert_eq!(&frame[..3], &[0xFF, 0xFB, 0x90]);
    }
}

#[test]
fn id3_tags_wrap_the_frames() {
    let png = kit::png(3, 2);
    let tagged = kit::mp3_with_id3v2(4, "Title", "Artist", Some(&png));
    assert_eq!(&tagged[..3], b"ID3");
    assert!(tagged.windows(4).any(|w| w == b"TIT2"));
    assert!(tagged.windows(4).any(|w| w == b"APIC"));
    let v1 = kit::mp3_with_id3v1(4, "Old");
    assert_eq!(&v1[v1.len() - 128..v1.len() - 125], b"TAG");
}

#[test]
fn png_signature_and_ihdr() {
    let png = kit::png(3, 2);
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(&png[12..16], b"IHDR");
    assert_eq!(be32(&png, 16), 3);
    assert_eq!(be32(&png, 20), 2);
}

#[test]
fn gif_and_bmp_headers() {
    let gif = kit::gif(4, 5);
    assert_eq!(&gif[..6], b"GIF89a");
    assert_eq!(u16::from_le_bytes([gif[6], gif[7]]), 4);
    let bmp = kit::bmp(4, 5);
    assert_eq!(&bmp[..2], b"BM");
    assert_eq!(u32::from_le_bytes(bmp[18..22].try_into().unwrap()), 4);
}

#[test]
fn midi_header() {
    let midi = kit::midi(500_000, 480, 1920, "Song");
    assert_eq!(&midi[..4], b"MThd");
    assert_eq!(&midi[8..10], &[0, 0]);
    assert_eq!(&midi[10..12], &[0, 1]);
    assert_eq!(&midi[14..18], b"MTrk");
}
