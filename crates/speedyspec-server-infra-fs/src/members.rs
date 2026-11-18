//! `<data_dir>/cluster/members/<pod>.json`: each pod's report, written only by that pod, without locks.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use speedyspec_server_domain::{ClusterMembers, MemberReport, StateError};

/// The member files on the shared volume. Writes replace this pod's file atomically (temp file, fsync, rename, fsync
/// the directory). A file that does not parse (a torn read on a network file system) is answered with the last good
/// copy this reader saw, and logged once until it parses again.
pub struct FsClusterMembers {
    dir: PathBuf,
    pod: String,
    last_good: Mutex<BTreeMap<String, MemberReport>>,
    unreadable: Mutex<BTreeSet<String>>,
}

impl FsClusterMembers {
    pub fn new(data_dir: &Path, pod: &str) -> FsClusterMembers {
        FsClusterMembers {
            dir: data_dir.join("cluster/members"),
            pod: pod.to_string(),
            last_good: Mutex::new(BTreeMap::new()),
            unreadable: Mutex::new(BTreeSet::new()),
        }
    }

    fn error(&self, path: &Path, err: impl ToString) -> StateError {
        StateError {
            path: path.to_path_buf(),
            message: err.to_string(),
        }
    }
}

impl ClusterMembers for FsClusterMembers {
    fn write(&self, report: &MemberReport) -> Result<(), StateError> {
        if report.pod != self.pod {
            return Err(self.error(
                &self.dir.join(format!("{}.json", report.pod)),
                format!("pod {} writes only its own member file", self.pod),
            ));
        }
        std::fs::create_dir_all(&self.dir).map_err(|err| self.error(&self.dir, err))?;
        let path = self.dir.join(format!("{}.json", self.pod));
        let tmp = self.dir.join(format!(".{}.json.tmp", self.pod));
        let json = serde_json::to_vec(report).map_err(|err| self.error(&path, err))?;
        let mut file = File::create(&tmp).map_err(|err| self.error(&tmp, err))?;
        file.write_all(&json).map_err(|err| self.error(&tmp, err))?;
        file.sync_all().map_err(|err| self.error(&tmp, err))?;
        drop(file);
        std::fs::rename(&tmp, &path).map_err(|err| self.error(&path, err))?;
        File::open(&self.dir)
            .and_then(|dir| dir.sync_all())
            .map_err(|err| self.error(&self.dir, err))?;
        Ok(())
    }

    fn read_all(&self) -> Result<Vec<MemberReport>, StateError> {
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(self.error(&self.dir, err)),
        };
        let mut last_good = self.last_good.lock().unwrap_or_else(|p| p.into_inner());
        let mut unreadable = self.unreadable.lock().unwrap_or_else(|p| p.into_inner());
        let mut reports = BTreeMap::new();
        for entry in entries {
            let entry = entry.map_err(|err| self.error(&self.dir, err))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(pod) = name.strip_suffix(".json").filter(|p| !p.starts_with('.')) else {
                continue;
            };
            let parsed = std::fs::read(entry.path())
                .map_err(|err| err.to_string())
                .and_then(|bytes| {
                    serde_json::from_slice::<MemberReport>(&bytes).map_err(|err| err.to_string())
                });
            match parsed {
                Ok(report) => {
                    unreadable.remove(pod);
                    last_good.insert(pod.to_string(), report.clone());
                    reports.insert(pod.to_string(), report);
                }
                Err(err) => {
                    if unreadable.insert(pod.to_string()) {
                        tracing::warn!(event = "member_unreadable", pod = %pod, error = %err, "keeping its last good report");
                    }
                    match last_good.get(pod) {
                        Some(report) => {
                            reports.insert(pod.to_string(), report.clone());
                        }
                        // Never parsed: this pod's state is unknown, so the caller must not act as if it were gone.
                        None => {
                            return Err(self.error(
                                &entry.path(),
                                format!("member {pod} has not been readable yet: {err}"),
                            ));
                        }
                    }
                }
            }
        }
        Ok(reports.into_values().collect())
    }
}
