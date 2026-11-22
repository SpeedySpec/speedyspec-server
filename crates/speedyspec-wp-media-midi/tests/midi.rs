use std::time::Duration;

use speedyspec_wp_media_contracts::{ContainerKind, ProbeLimits, ReadError, TagKey, TagSystem};
use speedyspec_wp_media_midi::parse;
use speedyspec_wp_media_testkit::{midi, vlq};

fn smf(format: u16, division: [u8; 2], tracks: &[Vec<u8>]) -> Vec<u8> {
    let mut out = b"MThd\0\0\0\x06".to_vec();
    out.extend_from_slice(&format.to_be_bytes());
    out.extend_from_slice(&(tracks.len() as u16).to_be_bytes());
    out.extend_from_slice(&division);
    for track in tracks {
        out.extend_from_slice(b"MTrk");
        out.extend_from_slice(&(track.len() as u32).to_be_bytes());
        out.extend_from_slice(track);
    }
    out
}

fn tempo(delta: u32, us: u32) -> Vec<u8> {
    [
        vlq(delta),
        vec![0xFF, 0x51, 0x03],
        us.to_be_bytes()[1..].to_vec(),
    ]
    .concat()
}

fn end(delta: u32) -> Vec<u8> {
    [vlq(delta), vec![0xFF, 0x2F, 0x00]].concat()
}

fn secs(d: Option<Duration>) -> f64 {
    d.expect("a duration").as_secs_f64()
}

#[test]
fn format0_duration() {
    let info = parse(&midi(500_000, 480, 1920, "Song"), &ProbeLimits::default()).unwrap();
    assert_eq!(info.container, ContainerKind::Midi);
    assert_eq!(info.mime_type.as_deref(), Some("audio/midi"));
    assert!((secs(info.duration) - 2.0).abs() < 1e-9);
    let title = info.tags.iter().find(|t| t.key == TagKey::Title).unwrap();
    assert_eq!(
        (title.values[0].as_str(), title.system),
        ("Song", TagSystem::Midi)
    );
    assert_eq!(
        info.raw.reader.as_ref().map(|(name, _)| name.as_str()),
        Some("midi")
    );
}

#[test]
fn tempo_change_mid_track() {
    let track = [tempo(0, 500_000), tempo(480, 250_000), end(480)].concat();
    let info = parse(&smf(0, [0x01, 0xE0], &[track]), &ProbeLimits::default()).unwrap();
    assert!((secs(info.duration) - 0.75).abs() < 1e-9);
}

#[test]
fn format1_merges_tempo_track() {
    let tempo_track = [tempo(0, 500_000), tempo(480, 250_000), end(0)].concat();
    let notes = [
        vlq(0),
        vec![0x90, 0x3C, 0x40],
        vlq(960),
        vec![0x80, 0x3C, 0x40],
        end(0),
    ]
    .concat();
    let info = parse(
        &smf(1, [0x01, 0xE0], &[tempo_track, notes]),
        &ProbeLimits::default(),
    )
    .unwrap();
    assert!((secs(info.duration) - 0.75).abs() < 1e-9);
}

#[test]
fn running_status_is_skipped_correctly() {
    let track = [
        vlq(0),
        vec![0x90, 0x3C, 0x40],
        vlq(240),
        vec![0x3C, 0x00],
        vlq(240),
        vec![0x3E, 0x40],
        vlq(480),
        vec![0x3E, 0x00],
        vlq(0),
        vec![0xF0, 0x03, 0x7E, 0x7F, 0xF7],
        end(0),
    ]
    .concat();
    let info = parse(&smf(0, [0x01, 0xE0], &[track]), &ProbeLimits::default()).unwrap();
    assert!((secs(info.duration) - 1.0).abs() < 1e-9);
}

#[test]
fn smpte_division() {
    let info = parse(&smf(0, [0xE7, 40], &[end(1000)]), &ProbeLimits::default()).unwrap();
    assert!((secs(info.duration) - 1.0).abs() < 1e-9);
}

#[test]
fn truncated_chunk() {
    let mut bytes = midi(500_000, 480, 1920, "Song");
    bytes.truncate(bytes.len() - 5);
    assert_eq!(
        parse(&bytes, &ProbeLimits::default()).unwrap_err(),
        ReadError::Truncated
    );
}

#[test]
fn vlq_too_long() {
    let track = [vec![0xFF, 0xFF, 0xFF, 0xFF, 0x7F], vec![0xFF, 0x2F, 0x00]].concat();
    assert_eq!(
        parse(&smf(0, [0x01, 0xE0], &[track]), &ProbeLimits::default()).unwrap_err(),
        ReadError::Truncated
    );
}

#[test]
fn not_midi_is_unrecognized() {
    assert_eq!(
        parse(b"RIFF....WAVE", &ProbeLimits::default()).unwrap_err(),
        ReadError::Unrecognized
    );
}
