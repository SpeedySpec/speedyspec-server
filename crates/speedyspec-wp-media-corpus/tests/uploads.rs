//! The formats the widened MIME list adds: every added row's corpus file is read as that row's kind, and the files in
//! `compat/media/uploads` read through WordPress's rules match what WordPress 7.1.5 and getID3 returned
//! (`compat/media/recorded-uploads.json`), except the paths `compat/media/upload-differences.json` lists.

mod common;

use speedyspec_wp_media::SniffingReader;
use speedyspec_wp_media::images;
use speedyspec_wp_media::mime::{ADDED, Kind};
use speedyspec_wp_media_contracts::{MediaReader, MediaSource, ProbeLimits, Stream};
use speedyspec_wp_media_ffmpeg::FfmpegReader;
use speedyspec_wp_media_midi::MidiReader;

#[test]
fn every_added_row_is_read() {
    let limits = ProbeLimits::default();
    let reader = SniffingReader::new(Box::new(MidiReader), Box::new(FfmpegReader::new(&limits)));
    let mut wrong = Vec::new();
    for row in ADDED {
        let path = common::compat().join(row.corpus);
        let ok = match row.kind {
            Kind::Image => {
                let head = std::fs::read(&path).unwrap();
                images::sniff(&head[..head.len().min(64)]).map(|t| t.mime) == Some(row.mime)
            }
            Kind::Audio | Kind::Video => {
                match reader.read(MediaSource::Path(path.clone()), &limits) {
                    Ok(info) => {
                        let video = info.streams.iter().any(|s| matches!(s, Stream::Video(_)));
                        let audio = info.streams.iter().any(|s| matches!(s, Stream::Audio(_)));
                        if row.kind == Kind::Video {
                            video
                        } else {
                            audio && !video
                        }
                    }
                    Err(_) => false,
                }
            }
        };
        if !ok {
            wrong.push(format!("{} ({}): {}", row.exts, row.mime, row.corpus));
        }
    }
    assert!(
        wrong.is_empty(),
        "rows not read as their kind:\n{}",
        wrong.join("\n")
    );
}

fn run() -> (Vec<String>, Vec<String>) {
    common::run(
        "recorded-uploads.json",
        "uploads",
        "upload-differences.json",
        Some("differences.json"),
    )
}

#[test]
fn uploads_match_wordpress_except_listed_differences() {
    let (mismatches, _) = run();
    assert!(
        mismatches.is_empty(),
        "{} mismatches:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
}

#[test]
fn every_upload_difference_is_still_needed() {
    let (_, stale) = run();
    assert!(
        stale.is_empty(),
        "upload-differences.json entries that no longer differ:\n{}",
        stale.join("\n")
    );
}
