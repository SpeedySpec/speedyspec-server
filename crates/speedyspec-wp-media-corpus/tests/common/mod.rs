//! The comparison every audio and video corpus test shares: each recorded file read with FFmpeg (or the MIDI reader)
//! through WordPress's rules must match what WordPress and getID3 returned, except the paths a differences file
//! lists, each with a reason in `DIFFERENCES.md`.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use speedyspec_wp_media::SniffingReader;
use speedyspec_wp_media::functions::{read_audio_metadata, read_video_metadata};
use speedyspec_wp_media_contracts::{ProbeLimits, TagSanitizer, TimeParser, Tree};
use speedyspec_wp_media_ffmpeg::FfmpegReader;
use speedyspec_wp_media_midi::MidiReader;

pub fn compat() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../compat/media")
}

struct Same;
impl TagSanitizer for Same {
    fn post(&self, value: &[u8]) -> Vec<u8> {
        value.to_vec()
    }
    fn post_deep(&self, value: &Tree) -> Tree {
        value.clone()
    }
}

/// PHP's strtotime for the ISO 8601 UTC times the corpus carries.
struct IsoTime;
impl TimeParser for IsoTime {
    fn strtotime(&self, _text: &[u8]) -> Option<i64> {
        None
    }
}

/// A tree as the recorder writes PHP values: binary strings as length and hash, short non-UTF-8 strings as hex.
fn plain(tree: &Tree) -> Value {
    match tree {
        Tree::Null => Value::Null,
        Tree::Bool(b) => json!(b),
        Tree::Int(i) => json!(i),
        Tree::Float(f) => json!(f),
        Tree::Str(s) => json!(s),
        Tree::Bytes(b) => match std::str::from_utf8(b) {
            Ok(s) => json!(s),
            Err(_) if b.len() <= 256 && !b.contains(&0) => {
                json!({"$latin1": b.iter().map(|x| format!("{x:02x}")).collect::<String>()})
            }
            Err(_) => {
                json!({"$bytes": {"len": b.len(), "sha256": Sha256::digest(b).iter().map(|b| format!("{b:02x}")).collect::<String>()}})
            }
        },
        Tree::List(l) => Value::Array(l.iter().map(plain).collect()),
        Tree::Map(m) => Value::Object(
            m.iter()
                .map(|(k, v)| (k.clone(), plain(v)))
                .collect::<Map<_, _>>(),
        ),
    }
}

struct Allowed(Vec<(String, String)>);

impl Allowed {
    fn load(differences: &str) -> Allowed {
        let list: Vec<Value> =
            serde_json::from_str(&std::fs::read_to_string(compat().join(differences)).unwrap())
                .unwrap();
        Allowed(
            list.iter()
                .map(|d| {
                    (
                        d["file"].as_str().unwrap().into(),
                        d["path"].as_str().unwrap().into(),
                    )
                })
                .collect(),
        )
    }

    /// The entries of `differences` that apply to every file (`"file": "*"`).
    fn wildcards(differences: &str) -> Allowed {
        let all = Allowed::load(differences);
        Allowed(all.0.into_iter().filter(|(f, _)| f == "*").collect())
    }
    /// Whether `path` of `file` may differ: a listed path matches itself and everything under it; `*` matches one
    /// segment, a trailing `**` everything.
    fn covers(&self, file: &str, path: &str) -> Option<usize> {
        self.0
            .iter()
            .position(|(f, p)| (f == "*" || f == file) && matches(p, path))
    }
}

fn matches(pattern: &str, path: &str) -> bool {
    let pat: Vec<&str> = pattern.split('.').collect();
    let seg: Vec<&str> = path.split('.').collect();
    for (i, p) in pat.iter().enumerate() {
        if *p == "**" {
            return true;
        }
        match seg.get(i) {
            Some(s) if p == s || *p == "*" => {}
            _ => return false,
        }
    }
    seg.len() >= pat.len()
}

fn equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            (x.as_f64().unwrap() - y.as_f64().unwrap()).abs() < 1e-9
        }
        _ => a == b,
    }
}

fn compare(
    file: &str,
    path: &str,
    expected: &Value,
    actual: Option<&Value>,
    allowed: &Allowed,
    used: &mut [bool],
    out: &mut Vec<String>,
) {
    if let Some(i) = allowed.covers(file, path) {
        used[i] |= actual.is_none_or(|a| !equal(expected, a));
        return;
    }
    match (expected, actual) {
        (Value::Object(e), Some(Value::Object(a))) => {
            for (k, ev) in e {
                compare(
                    file,
                    &format!("{path}.{k}"),
                    ev,
                    a.get(k),
                    allowed,
                    used,
                    out,
                );
            }
            for (k, av) in a {
                if !e.contains_key(k) {
                    let p = format!("{path}.{k}");
                    match allowed.covers(file, &p) {
                        Some(i) => used[i] = true,
                        None => {
                            out.push(format!("{file}: {p}: not in WordPress's output, ours {av}"))
                        }
                    }
                }
            }
        }
        (e, Some(a)) if equal(e, a) => {}
        (e, a) => out.push(format!(
            "{file}: {path}: WordPress {e}, ours {}",
            a.map_or("(absent)".into(), |v| v.to_string())
        )),
    }
}

/// Replays `recording` (files under `dir`, both in `compat/media/`) and returns the mismatches `differences` doesn't
/// cover, and its entries nothing needed. `shared`'s entries for every file (`"file": "*"`, the general reasons such
/// as bitrates) apply too, without being reported as unneeded.
pub fn run(
    recording: &str,
    dir: &str,
    differences: &str,
    shared: Option<&str>,
) -> (Vec<String>, Vec<String>) {
    let recorded: Value =
        serde_json::from_str(&std::fs::read_to_string(compat().join(recording)).unwrap()).unwrap();
    let own = Allowed::load(differences);
    let own_len = own.0.len();
    let mut allowed = own;
    if let Some(shared) = shared {
        allowed.0.extend(Allowed::wildcards(shared).0);
    }
    let mut used = vec![false; allowed.0.len()];
    let limits = ProbeLimits::default();
    let reader = SniffingReader::new(Box::new(MidiReader), Box::new(FfmpegReader::new(&limits)));
    let mut mismatches = Vec::new();
    for (name, rec) in recorded["files"].as_object().unwrap() {
        let path = compat().join(dir).join(name);
        let read = if rec["kind"] == "audio" {
            read_audio_metadata
        } else {
            read_video_metadata
        };
        let outcome = read(&path, &reader, &limits, &Same, &IsoTime).expect("corpus files exist");
        let ours = json!({
            "metadata": plain(&outcome.metadata),
            "file_format": outcome.file_format.as_ref().map(|f| String::from_utf8_lossy(f).into_owned()),
        });
        // WordPress returns [] for a file whose data has nothing to copy; ours is an empty map.
        let expected = json!({
            "metadata": if rec["metadata"] == json!([]) { json!({}) } else { rec["metadata"].clone() },
            "file_format": rec["file_format"].clone(),
        });
        compare(
            name,
            "",
            &expected,
            Some(&ours),
            &allowed,
            &mut used,
            &mut mismatches,
        );
    }
    let stale = allowed.0[..own_len]
        .iter()
        .zip(&used)
        .filter(|(_, u)| !**u)
        .map(|((f, p), _)| format!("{f} {p}"))
        .collect();
    (mismatches, stale)
}
