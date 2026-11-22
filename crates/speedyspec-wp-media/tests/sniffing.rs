use std::cell::RefCell;
use std::fs::File;
use std::os::fd::{AsRawFd, OwnedFd};
use std::rc::Rc;

use speedyspec_wp_media::SniffingReader;
use speedyspec_wp_media_contracts::{
    ContainerKind, MediaInfo, MediaReader, MediaSource, ProbeLimits, RawViews, ReadError,
};
use speedyspec_wp_media_testkit as kit;

struct Recording {
    name: &'static str,
    calls: Rc<RefCell<Vec<&'static str>>>,
}

impl MediaReader for Recording {
    fn read(&self, _source: MediaSource, _limits: &ProbeLimits) -> Result<MediaInfo, ReadError> {
        self.calls.borrow_mut().push(self.name);
        Ok(MediaInfo {
            container: ContainerKind::Other(self.name.into()),
            mime_type: None,
            duration: None,
            bitrate: None,
            file_size: None,
            created_at: None,
            streams: vec![],
            tags: vec![],
            pictures: vec![],
            raw: RawViews::default(),
        })
    }
}

fn reader() -> (SniffingReader, Rc<RefCell<Vec<&'static str>>>) {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let reader = SniffingReader::new(
        Box::new(Recording {
            name: "midi",
            calls: calls.clone(),
        }),
        Box::new(Recording {
            name: "general",
            calls: calls.clone(),
        }),
    );
    (reader, calls)
}

fn read_path(bytes: &[u8]) -> Vec<&'static str> {
    let (reader, calls) = reader();
    let file = kit::write_temp(bytes, "bin");
    reader
        .read(
            MediaSource::Path(file.path().to_path_buf()),
            &ProbeLimits::default(),
        )
        .unwrap();
    calls.borrow().clone()
}

#[test]
fn midi_magic_goes_to_midi() {
    assert_eq!(read_path(&kit::midi(500_000, 480, 480, "x")), vec!["midi"]);
}

#[test]
fn anything_else_goes_to_general() {
    assert_eq!(read_path(&kit::wav_pcm16(0.1, 8000, 1)), vec!["general"]);
}

#[test]
fn short_file_goes_to_general() {
    assert_eq!(read_path(b"MT"), vec!["general"]);
}

#[test]
fn fd_offset_is_untouched() {
    let file = kit::write_temp(&kit::midi(500_000, 480, 480, "x"), "mid");
    let fd: OwnedFd = File::open(file.path()).unwrap().into();
    let raw = fd.as_raw_fd();
    let dup = fd.try_clone().unwrap();
    let (reader, calls) = reader();
    reader
        .read(MediaSource::Fd(fd), &ProbeLimits::default())
        .unwrap();
    assert_eq!(*calls.borrow(), vec!["midi"]);
    let _ = raw;
    assert_eq!(
        unsafe { libc::lseek(dup.as_raw_fd(), 0, libc::SEEK_CUR) },
        0
    );
}
