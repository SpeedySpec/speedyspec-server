//! Every file a fuzzer ever crashed or hung the reader with (minimized, in `tests/regressions/`) must now read to
//! some result, quickly. Fuzzing runs on a schedule (`.github/workflows/fuzz.yml`); this replay runs on every PR.

use std::time::{Duration, Instant};

use speedyspec_wp_media_contracts::{MediaReader, MediaSource, ProbeLimits};
use speedyspec_wp_media_ffmpeg::FfmpegReader;

#[test]
fn every_regression_reads_to_a_result() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/regressions");
    let limits = ProbeLimits { deadline: Duration::from_secs(1), ..ProbeLimits::default() };
    let reader = FfmpegReader::new(&limits);
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.file_name().is_some_and(|n| n == ".gitkeep") {
            continue;
        }
        let started = Instant::now();
        let _ = reader.read(MediaSource::Path(path.clone()), &limits);
        assert!(started.elapsed() < Duration::from_secs(2), "{} took {:?}", path.display(), started.elapsed());
    }
}
