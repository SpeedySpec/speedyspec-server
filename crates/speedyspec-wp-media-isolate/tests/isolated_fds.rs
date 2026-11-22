//! Every probe child is reaped and every descriptor closed: alone in its own test binary, so no other test opens
//! descriptors while it counts. Linux only: it reads /proc/self/fd.
#![cfg(target_os = "linux")]

use std::ffi::OsString;
use std::path::PathBuf;

use speedyspec_wp_media_contracts::{MediaReader, MediaSource, ProbeLimits};
use speedyspec_wp_media_isolate::{IsolatedReader, ProbeCommand};
use speedyspec_wp_media_testkit as kit;

fn reader() -> IsolatedReader {
    IsolatedReader::new(ProbeCommand {
        program: PathBuf::from(env!("CARGO_BIN_EXE_speedyspec-media-probe")),
        args: Vec::<OsString>::new(),
    })
}

#[test]
fn no_zombies_or_fd_leaks() {
    fn fds() -> usize {
        std::fs::read_dir("/proc/self/fd").unwrap().count()
    }
    let file = kit::write_temp(&kit::wav_pcm16(1.0, 8000, 1), "wav");
    let path = file.path().to_path_buf();
    let before = fds();
    std::thread::scope(|scope| {
        for _ in 0..8 {
            let path = path.clone();
            scope.spawn(move || {
                for _ in 0..7 {
                    reader()
                        .read(MediaSource::Path(path.clone()), &ProbeLimits::default())
                        .unwrap();
                }
            });
        }
    });
    assert_eq!(fds(), before);
    let mut status = 0;
    assert!(
        unsafe { libc::waitpid(-1, &mut status, libc::WNOHANG) } <= 0,
        "a child was left unreaped"
    );
}
