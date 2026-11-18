//! A lease in one file, for several processes on **one host** (the multi-process tests). It relies on one shared
//! clock and on `flock`, so it must never be used on a shared network volume; the binary accepts it only when built
//! with `test-hooks`.

use std::fs::OpenOptions;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use speedyspec_server_domain::{LeaderView, Leadership, LeaseTimings};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct LeaseRecord {
    holder: String,
    renewed_ms: u64,
    transitions: u64,
}

#[derive(Default)]
struct Held {
    /// When this pod last renewed successfully, while it holds the lease.
    renewed: Option<Instant>,
    /// The record as last read.
    seen: LeaseRecord,
    watchers: Vec<Sender<LeaderView>>,
    last_sent: Option<LeaderView>,
}

/// See the module docs. `start` spawns the renewing thread; dropping the last handle stops it.
pub struct FileLeaseLeadership {
    pod: String,
    timings: LeaseTimings,
    held: Arc<Mutex<Held>>,
    paused: Arc<AtomicBool>,
    stopped: Arc<AtomicBool>,
    path: PathBuf,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn view_of(pod: &str, timings: &LeaseTimings, held: &Held) -> LeaderView {
    let is_leader = held.seen.holder == pod
        && held
            .renewed
            .is_some_and(|at| at.elapsed() < timings.renew_deadline);
    let leader = (!held.seen.holder.is_empty()
        && now_ms().saturating_sub(held.seen.renewed_ms) < timings.lease.as_millis() as u64)
        .then(|| held.seen.holder.clone());
    LeaderView {
        is_leader,
        epoch: held.seen.transitions,
        leader,
    }
}

/// Reads, changes and writes the record under an exclusive lock on the file itself.
fn with_record(
    path: &Path,
    change: impl FnOnce(&mut LeaseRecord) -> bool,
) -> std::io::Result<LeaseRecord> {
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    file.lock()?;
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    let mut record: LeaseRecord = serde_json::from_str(&text).unwrap_or_default();
    if change(&mut record) {
        let json = serde_json::to_vec(&record).map_err(std::io::Error::other)?;
        file.set_len(0)?;
        file.rewind()?;
        file.write_all(&json)?;
        file.sync_all()?;
    }
    Ok(record)
}

impl FileLeaseLeadership {
    pub fn start(path: &Path, pod: &str, timings: LeaseTimings) -> FileLeaseLeadership {
        let lease = FileLeaseLeadership {
            pod: pod.to_string(),
            timings,
            held: Arc::new(Mutex::new(Held::default())),
            paused: Arc::new(AtomicBool::new(false)),
            stopped: Arc::new(AtomicBool::new(false)),
            path: path.to_path_buf(),
        };
        let (pod, path, held, paused, stopped) = (
            lease.pod.clone(),
            lease.path.clone(),
            lease.held.clone(),
            lease.paused.clone(),
            lease.stopped.clone(),
        );
        std::thread::spawn(move || {
            while !stopped.load(Ordering::SeqCst) {
                if !paused.load(Ordering::SeqCst) {
                    tick(&path, &pod, &timings, &held);
                }
                notify(&pod, &timings, &held);
                std::thread::sleep(timings.retry);
            }
        });
        lease
    }

    /// Under `test-hooks` tests: stops (or resumes) renewing, as a stalled process would.
    pub fn pause_renewals(&self, paused: bool) {
        self.paused.store(paused, Ordering::SeqCst);
    }
}

fn tick(path: &Path, pod: &str, timings: &LeaseTimings, held: &Mutex<Held>) {
    let lease_ms = timings.lease.as_millis() as u64;
    let attempt = Instant::now();
    let result = with_record(path, |record| {
        let now = now_ms();
        if record.holder == pod {
            record.renewed_ms = now;
            return true;
        }
        if record.holder.is_empty() || now.saturating_sub(record.renewed_ms) >= lease_ms {
            record.holder = pod.to_string();
            record.renewed_ms = now;
            record.transitions += 1;
            return true;
        }
        false
    });
    let mut held = held.lock().unwrap_or_else(|p| p.into_inner());
    match result {
        Ok(record) => {
            held.renewed = (record.holder == pod).then_some(attempt);
            held.seen = record;
        }
        Err(err) => tracing::warn!(event = "lease_unreadable", error = %err),
    }
}

fn notify(pod: &str, timings: &LeaseTimings, held: &Mutex<Held>) {
    let mut held = held.lock().unwrap_or_else(|p| p.into_inner());
    let view = view_of(pod, timings, &held);
    if held.last_sent.as_ref() != Some(&view) {
        held.watchers.retain(|tx| tx.send(view.clone()).is_ok());
        held.last_sent = Some(view);
    }
}

impl Leadership for FileLeaseLeadership {
    fn view(&self) -> LeaderView {
        view_of(
            &self.pod,
            &self.timings,
            &self.held.lock().unwrap_or_else(|p| p.into_inner()),
        )
    }

    fn changed(&self) -> Receiver<LeaderView> {
        let (tx, rx) = channel();
        self.held
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .watchers
            .push(tx);
        rx
    }

    fn step_down(&self) {
        self.paused.store(true, Ordering::SeqCst);
        let pod = self.pod.clone();
        let result = with_record(&self.path, |record| {
            if record.holder == pod {
                record.holder.clear();
                record.renewed_ms = 0;
                return true;
            }
            false
        });
        let mut held = self.held.lock().unwrap_or_else(|p| p.into_inner());
        held.renewed = None;
        if let Ok(record) = result {
            held.seen = record;
        }
        drop(held);
        notify(&self.pod, &self.timings, &self.held);
    }
}

impl Drop for FileLeaseLeadership {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
    }
}
