//! `IsolatedReader`: runs the readers in a limited child process and kills it at the deadline (Protection Proxy).

use std::ffi::OsString;
use std::io::Read;
use std::os::fd::OwnedFd;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use speedyspec_wp_media_contracts::{
    Limit, MediaInfo, MediaReader, MediaSource, ProbeLimits, ReadError,
};

use crate::{child, log, wire};

/// Extra wall time after the child's own deadline before the parent kills it.
const GRACE: Duration = Duration::from_millis(250);
const POLL: Duration = Duration::from_millis(2);
const STDERR_KEPT: usize = 4096;

/// How to start the probe child: the runtime binary with `media-probe`, or the test probe binary.
#[derive(Clone, Debug)]
pub struct ProbeCommand {
    pub program: PathBuf,
    pub args: Vec<OsString>,
}

/// Reads untrusted files in a fresh child per read: no environment, the file as stdin and nothing else open, resource
/// limits, its own process group, and on Linux a seccomp filter. The child is always reaped before `read` returns.
pub struct IsolatedReader {
    command: ProbeCommand,
}

impl IsolatedReader {
    pub fn new(command: ProbeCommand) -> IsolatedReader {
        IsolatedReader { command }
    }
}

fn rlimit(resource: i32, value: u64) {
    let limit = libc::rlimit {
        rlim_cur: value as libc::rlim_t,
        rlim_max: value as libc::rlim_t,
    };
    // SAFETY: async-signal-safe; a failure leaves the inherited limit, which the parent's watch backs up.
    unsafe { libc::setrlimit(resource as _, &limit) };
}

/// Marks every descriptor above stderr close-on-exec, so nothing the worker opened without `O_CLOEXEC` (PHP's files,
/// database sockets) reaches the child: exec then leaves it only stdin, stdout and stderr. Marking rather than closing
/// keeps the standard library's own exec-error pipe working. Async-signal-safe: one syscall, or `fcntl` in a loop.
fn cloexec_inherited() {
    #[cfg(target_os = "linux")]
    {
        const CLOSE_RANGE_CLOEXEC: libc::c_uint = 1 << 2;
        // SAFETY: close_range only changes descriptor flags in this (forked) process.
        if unsafe { libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, CLOSE_RANGE_CLOEXEC) } == 0
        {
            return;
        }
    }
    // SAFETY: getdtablesize and fcntl are async-signal-safe and only touch this process's descriptor table.
    let top = unsafe { libc::getdtablesize() }.clamp(3, 65536);
    for fd in 3..top {
        unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
    }
}

fn drain(
    mut pipe: impl Read + Send + 'static,
    cap: usize,
    over: Option<Arc<AtomicBool>>,
) -> JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut kept = Vec::new();
        let mut chunk = [0u8; 65536];
        loop {
            match pipe.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if kept.len() + n > cap {
                        match &over {
                            Some(flag) => {
                                flag.store(true, Ordering::SeqCst);
                                break;
                            }
                            None => {
                                kept.extend_from_slice(&chunk[..cap.saturating_sub(kept.len())])
                            }
                        }
                    } else {
                        kept.extend_from_slice(&chunk[..n]);
                    }
                }
            }
        }
        kept
    })
}

fn kill_group(child: &Child) {
    // SAFETY: the child leads its own process group (setpgid in pre_exec).
    unsafe { libc::killpg(child.id() as libc::pid_t, libc::SIGKILL) };
}

enum Stop {
    Exited(ExitStatus),
    TimedOut,
    /// Only macOS watches resident memory; Linux enforces RLIMIT_AS in the kernel.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Memory,
    Output,
}

fn first_line(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr)
        .lines()
        .next()
        .unwrap_or("")
        .to_owned()
}

impl IsolatedReader {
    fn run(&self, fd: OwnedFd, path: &str, limits: &ProbeLimits) -> Result<MediaInfo, ReadError> {
        let cpu = limits.deadline.as_secs() + 2;
        let memory = limits.max_memory;
        let mut command = Command::new(&self.command.program);
        command
            .args(&self.command.args)
            .args(child::limit_args(limits))
            .env_clear()
            .stdin(Stdio::from(fd))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // SAFETY: only async-signal-safe calls between fork and exec.
        // The RLIMIT_* constants are c_int on some targets and u32 on others (glibc); the casts make one signature.
        #[allow(clippy::unnecessary_cast)]
        unsafe {
            command.pre_exec(move || {
                libc::setpgid(0, 0);
                cloexec_inherited();
                rlimit(libc::RLIMIT_CPU as i32, cpu);
                rlimit(libc::RLIMIT_NOFILE as i32, 16);
                rlimit(libc::RLIMIT_FSIZE as i32, 0);
                rlimit(libc::RLIMIT_CORE as i32, 0);
                #[cfg(target_os = "linux")]
                rlimit(libc::RLIMIT_AS as i32, memory);
                #[cfg(not(target_os = "linux"))]
                rlimit(libc::RLIMIT_DATA as i32, memory);
                Ok(())
            });
        }
        let started = Instant::now();
        let mut child = command
            .spawn()
            .map_err(|e| ReadError::Crashed(format!("spawning the probe: {e}")))?;
        let over = Arc::new(AtomicBool::new(false));
        let out_cap = usize::try_from(limits.max_picture_bytes + (8 << 20)).unwrap_or(usize::MAX);
        let stdout = drain(
            child.stdout.take().expect("piped"),
            out_cap,
            Some(over.clone()),
        );
        let stderr = drain(child.stderr.take().expect("piped"), STDERR_KEPT, None);

        let give_up = limits.deadline + GRACE;
        let stop = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Stop::Exited(status),
                Ok(None) => {}
                Err(_) => {
                    kill_group(&child);
                    break Stop::Exited(
                        child.wait().unwrap_or(ExitStatus::from_raw(libc::SIGKILL)),
                    );
                }
            }
            if over.load(Ordering::SeqCst) {
                kill_group(&child);
                let _ = child.wait();
                break Stop::Output;
            }
            if started.elapsed() > give_up {
                kill_group(&child);
                let _ = child.wait();
                break Stop::TimedOut;
            }
            #[cfg(target_os = "macos")]
            if crate::rss::resident(child.id()).is_some_and(|rss| rss > memory) {
                kill_group(&child);
                let _ = child.wait();
                break Stop::Memory;
            }
            std::thread::sleep(POLL);
        };
        let out = stdout.join().unwrap_or_default();
        let err = stderr.join().unwrap_or_default();
        let elapsed = started.elapsed().as_millis().to_string();

        let fail = |event: &str, detail: &str, error: ReadError| {
            log::event(
                event,
                &[("path", path), ("detail", detail), ("elapsed_ms", &elapsed)],
            );
            Err(error)
        };
        let status = match stop {
            Stop::TimedOut => {
                return fail("media_probe_timed_out", "deadline", ReadError::TimedOut);
            }
            Stop::Memory => {
                return fail(
                    "media_probe_memory",
                    "resident memory",
                    ReadError::LimitExceeded(Limit::Memory),
                );
            }
            Stop::Output => {
                return fail(
                    "media_probe_output",
                    "output too large",
                    ReadError::LimitExceeded(Limit::Output),
                );
            }
            Stop::Exited(status) => status,
        };
        if let Some(signal) = status.signal() {
            let detail = format!("signal {signal}: {}", first_line(&err));
            let error = match signal {
                libc::SIGXCPU => ReadError::LimitExceeded(Limit::Cpu),
                libc::SIGSYS => ReadError::Crashed("seccomp".into()),
                libc::SIGKILL => ReadError::LimitExceeded(Limit::Memory),
                libc::SIGABRT if String::from_utf8_lossy(&err).contains("memory allocation of") => {
                    ReadError::LimitExceeded(Limit::Memory)
                }
                _ => ReadError::Crashed(format!("signal {signal}")),
            };
            return fail("media_probe_crashed", &detail, error);
        }
        if status.code() != Some(0) {
            let detail = format!("exit {:?}: {}", status.code(), first_line(&err));
            return fail(
                "media_probe_crashed",
                &detail,
                ReadError::Crashed(detail.clone()),
            );
        }
        let payload = match out
            .get(..4)
            .map(|n| u32::from_le_bytes([n[0], n[1], n[2], n[3]]) as usize)
        {
            Some(len) if out.len() == 4 + len => &out[4..],
            _ => {
                return fail(
                    "media_probe_crashed",
                    "malformed output",
                    ReadError::Crashed("malformed output".into()),
                );
            }
        };
        match wire::decode(payload) {
            Ok(result) => result,
            Err(e) => fail(
                "media_probe_crashed",
                &e.to_string(),
                ReadError::Crashed(e.to_string()),
            ),
        }
    }
}

impl MediaReader for IsolatedReader {
    fn read(&self, source: MediaSource, limits: &ProbeLimits) -> Result<MediaInfo, ReadError> {
        let (fd, path) = match source {
            MediaSource::Path(path) => (open(&path)?, path.display().to_string()),
            MediaSource::Fd(fd) => (fd, String::new()),
        };
        regular(&fd)?;
        self.run(fd, &path, limits)
    }
}

fn open(path: &Path) -> Result<OwnedFd, ReadError> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map(OwnedFd::from)
        .map_err(|e| match e.raw_os_error() {
            Some(libc::ENOENT) | Some(libc::ENOTDIR) => ReadError::NotFound,
            Some(libc::ELOOP) => ReadError::Io("the path is a symlink".into()),
            _ => ReadError::Io(e.to_string()),
        })
}

fn regular(fd: &OwnedFd) -> Result<(), ReadError> {
    let file = std::fs::File::from(fd.try_clone().map_err(|e| ReadError::Io(e.to_string()))?);
    match file.metadata() {
        Ok(meta) if meta.file_type().is_file() => Ok(()),
        Ok(_) => Err(ReadError::Io("not a regular file".into())),
        Err(e) => Err(ReadError::Io(e.to_string())),
    }
}
