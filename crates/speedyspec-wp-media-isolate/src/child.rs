//! The probe child: reads the file on stdin and writes the outcome to stdout.

use std::ffi::OsString;
use std::io::Write;
use std::os::fd::{AsFd, OwnedFd};
use std::process::ExitCode;
use std::time::Duration;

use speedyspec_wp_media::SniffingReader;
use speedyspec_wp_media_contracts::{MediaReader, MediaSource, ProbeLimits};
use speedyspec_wp_media_ffmpeg::FfmpegReader;
use speedyspec_wp_media_midi::MidiReader;

use crate::wire;

/// The child's arguments for `limits` (the parent passes these; `run` parses them).
pub fn limit_args(limits: &ProbeLimits) -> Vec<OsString> {
    [
        ("--max-bytes", limits.max_bytes),
        ("--deadline-ms", limits.deadline.as_millis() as u64),
        ("--max-alloc", limits.max_alloc),
        ("--max-streams", u64::from(limits.max_streams)),
        ("--max-picture-bytes", limits.max_picture_bytes),
        ("--max-tag-bytes", limits.max_tag_bytes),
    ]
    .into_iter()
    .flat_map(|(name, value)| [OsString::from(name), OsString::from(value.to_string())])
    .collect()
}

fn parse(args: &[OsString]) -> Option<(ProbeLimits, Option<String>)> {
    let mut limits = ProbeLimits::default();
    let mut seen = 0u8;
    let mut simulate = None;
    let mut it = args.iter();
    while let Some(name) = it.next() {
        let value = it.next()?.to_str()?;
        let number = || value.parse::<u64>().ok();
        match name.to_str()? {
            "--max-bytes" => (limits.max_bytes, seen) = (number()?, seen | 1),
            "--deadline-ms" => (limits.deadline, seen) = (Duration::from_millis(number()?), seen | 2),
            "--max-alloc" => (limits.max_alloc, seen) = (number()?, seen | 4),
            "--max-streams" => (limits.max_streams, seen) = (u32::try_from(number()?).ok()?, seen | 8),
            "--max-picture-bytes" => (limits.max_picture_bytes, seen) = (number()?, seen | 16),
            "--max-tag-bytes" => (limits.max_tag_bytes, seen) = (number()?, seen | 32),
            "--simulate" if cfg!(feature = "test-modes") => simulate = Some(value.to_owned()),
            _ => return None,
        }
    }
    (seen == 63).then_some((limits, simulate))
}

#[cfg(feature = "test-modes")]
fn simulate(mode: &str) {
    match mode {
        "loop" => loop {
            std::hint::spin_loop();
        },
        "alloc" => {
            let mut hoard: Vec<Vec<u8>> = Vec::new();
            loop {
                hoard.push(vec![1u8; 64 << 20]);
            }
        }
        "crash" => std::process::abort(),
        "garbage" => {
            let _ = std::io::stdout().write_all(b"\x05\x00\x00\x00not-wire");
            std::process::exit(0);
        }
        _ => {}
    }
}

/// The child's main: parse the limits, lock the process down, read stdin, write the outcome. Exit 0 whenever an
/// outcome (even a read error) was written; 2 on bad arguments.
pub fn run(args: &[OsString]) -> ExitCode {
    let Some((limits, _simulate)) = parse(args) else {
        eprintln!("speedyspec media-probe: bad arguments");
        return ExitCode::from(2);
    };
    let reader = SniffingReader::new(Box::new(MidiReader), Box::new(FfmpegReader::new(&limits)));
    let stdin: OwnedFd = match std::io::stdin().as_fd().try_clone_to_owned() {
        Ok(fd) => fd,
        Err(err) => {
            eprintln!("speedyspec media-probe: stdin: {err}");
            return ExitCode::from(2);
        }
    };
    crate::sandbox::enter();
    #[cfg(feature = "test-modes")]
    if let Some(mode) = &_simulate {
        simulate(mode);
    }
    let result = reader.read(MediaSource::Fd(stdin), &limits);
    let payload = wire::encode(&result);
    let mut out = std::io::stdout().lock();
    let written = out
        .write_all(&(payload.len() as u32).to_le_bytes())
        .and_then(|()| out.write_all(&payload))
        .and_then(|()| out.flush());
    if written.is_err() {
        return ExitCode::from(3);
    }
    ExitCode::SUCCESS
}
