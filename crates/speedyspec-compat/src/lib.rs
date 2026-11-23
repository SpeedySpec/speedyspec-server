//! Recorded compatibility.
//!
//! An area's behaviour is pinned by a catalogue of scenarios (`compat/<area>/scenarios.json`, a [`Catalogue`]). The
//! recorder (`cargo run -p speedyspec-compat -- record <area> --wordpress <version>`) runs every scenario against
//! WordPress's original code, fetched from the pinned release, and writes a [`Fixture`]. The replayer, [`replay`],
//! runs the same scenarios through a [`ScenarioRunner`] (the port's side) and compares every step to the fixture,
//! exactly. A difference is a [`Mismatch`] unless `compat/<area>/breaks.json` lists that step as a documented break.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

mod mirror;
mod php;
mod record;

pub use mirror::ReleaseMirror;
pub use php::{DRIVER, PhpRunner};

/// The WordPress functions the object cache calls, declared the same on both sides of `compat/object-cache`.
pub const OBJECT_CACHE_STUBS: &str = include_str!("object_cache_stubs.php");
pub use record::{Source, area_runner, record, record_for, wordpress_release};

/// A catalogue of scenarios for one area.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Catalogue {
    pub area: String,
    pub scenarios: Vec<Scenario>,
}

/// A sequence of calls, run in a fresh request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scenario {
    pub name: String,
    /// Recorded on a multisite install, into the `-multisite` fixture, and replayed on a multisite port.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub multisite: bool,
    pub steps: Vec<Step>,
}

/// One call: a function, `Class::method`, `$var->method` on a value kept with `as`, `new WP_Hook`, or a
/// `global:<op>` on a global. An argument is a JSON value, a callback fixture (`{ "callback": .., "with": .. }`) or a
/// kept value (`{ "var": .. }`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub call: String,
    #[serde(default)]
    pub args: Vec<Value>,
    /// Keeps the step's value under this name, for later steps.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "as")]
    pub keep_as: Option<String>,
}

/// What one area recorded from one WordPress release.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fixture {
    pub wordpress: String,
    pub results: BTreeMap<String, Vec<StepResult>>,
}

/// What one step did.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StepResult {
    pub returned: Value,
    pub invocations: Vec<Invocation>,
    pub globals: Globals,
    /// The area's own snapshot after the step (`SpeedySpecCompat::$state`), when its support file sets one.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub state: Value,
}

/// One callback fixture's invocation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Invocation {
    pub fixture: String,
    pub args: Value,
    /// What a `record_current_filter` fixture saw.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed: Option<Value>,
}

/// The hook globals after a step, normalised (ordered pairs, labelled callbacks).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Globals {
    pub wp_filter: Value,
    pub wp_actions: Value,
    pub wp_filters: Value,
    pub wp_current_filter: Value,
}

/// The documented breaks of an area.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Breaks {
    pub breaks: Vec<Break>,
}

/// A step allowed to differ from WordPress (every step of the scenario when `step` is absent).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Break {
    pub scenario: String,
    #[serde(default)]
    pub step: Option<usize>,
    pub why: String,
}

impl Breaks {
    fn allows(&self, scenario: &str, step: usize) -> bool {
        self.breaks
            .iter()
            .any(|b| b.scenario == scenario && b.step.is_none_or(|s| s == step))
    }
}

/// Runs scenarios on one side (WordPress's code, or a port).
pub trait ScenarioRunner {
    fn run(&mut self, scenario: &Scenario) -> Result<Vec<StepResult>, String>;
}

/// A scenario whose replay differs from the fixture: the first differing step.
#[derive(Clone, Debug, PartialEq)]
pub struct Mismatch {
    pub scenario: String,
    pub step: usize,
    pub expected: Option<StepResult>,
    pub actual: Option<StepResult>,
    pub detail: String,
}

impl fmt::Display for Mismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}, step {}: {}", self.scenario, self.step, self.detail)
    }
}

/// Where the catalogues and fixtures live: `$SPEEDYSPEC_COMPAT_DIR`, or the repository's `compat/` when this crate
/// is built inside the speedyspec-server repository. Without either (a crate built from crates.io), an error saying
/// to set `SPEEDYSPEC_COMPAT_DIR`.
pub fn try_compat_dir() -> Result<PathBuf, String> {
    if let Some(dir) = std::env::var_os("SPEEDYSPEC_COMPAT_DIR") {
        return Ok(PathBuf::from(dir));
    }
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../compat");
    if repository.is_dir() {
        Ok(repository)
    } else {
        Err("no compat catalogue: set SPEEDYSPEC_COMPAT_DIR".to_owned())
    }
}

/// [`try_compat_dir`] for the recording and replaying tools, which cannot go on without the catalogue: panics with
/// its error when there is none.
pub fn compat_dir() -> PathBuf {
    try_compat_dir().unwrap_or_else(|e| panic!("{e}"))
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// The fixture file of `version`: `wordpress-<version>.json`, or `wordpress-<version>-multisite.json` for the
/// scenarios marked `multisite`.
pub fn fixture_name(version: &str, multisite: bool) -> String {
    if multisite {
        format!("wordpress-{version}-multisite.json")
    } else {
        format!("wordpress-{version}.json")
    }
}

/// The catalogue, the fixture of the pinned WordPress release, and the breaks of `area`: its single-site scenarios.
pub fn load(area: &str) -> Result<(Catalogue, Fixture, Breaks), String> {
    load_for(area, false)
}

/// The catalogue's scenarios recorded single site (`multisite` false) or on multisite (true), their fixture, and the
/// breaks of `area`.
pub fn load_for(area: &str, multisite: bool) -> Result<(Catalogue, Fixture, Breaks), String> {
    let dir = compat_dir().join(area);
    let mut catalogue: Catalogue = read_json(&dir.join("scenarios.json"))?;
    catalogue.scenarios.retain(|s| s.multisite == multisite);
    let source: Source = read_json(&dir.join("source.json"))?;
    let fixture = read_json(&dir.join(fixture_name(&source.wordpress, multisite)))?;
    let breaks_path = dir.join("breaks.json");
    let breaks = if breaks_path.exists() {
        read_json(&breaks_path)?
    } else {
        Breaks::default()
    };
    Ok((catalogue, fixture, breaks))
}

/// Replays `area`'s single-site catalogue through `runner` against its fixture.
pub fn replay(area: &str, runner: &mut dyn ScenarioRunner) -> Vec<Mismatch> {
    replay_for(area, false, runner)
}

/// Replays `area`'s single-site or multisite scenarios through `runner` against their fixture.
pub fn replay_for(area: &str, multisite: bool, runner: &mut dyn ScenarioRunner) -> Vec<Mismatch> {
    match load_for(area, multisite) {
        Ok((catalogue, fixture, breaks)) => replay_with(&catalogue, &fixture, &breaks, runner),
        Err(why) => vec![Mismatch {
            scenario: format!("<{area}>"),
            step: 0,
            expected: None,
            actual: None,
            detail: format!("the compatibility files cannot be loaded: {why}"),
        }],
    }
}

/// Replays `catalogue` through `runner`, comparing each step to `fixture` exactly; at most one mismatch per scenario.
pub fn replay_with(
    catalogue: &Catalogue,
    fixture: &Fixture,
    breaks: &Breaks,
    runner: &mut dyn ScenarioRunner,
) -> Vec<Mismatch> {
    let mut mismatches = Vec::new();
    for scenario in &catalogue.scenarios {
        let mismatch = |step: usize,
                        expected: Option<&StepResult>,
                        actual: Option<&StepResult>,
                        detail: String| {
            Mismatch {
                scenario: scenario.name.clone(),
                step,
                expected: expected.cloned(),
                actual: actual.cloned(),
                detail,
            }
        };
        let Some(expected) = fixture.results.get(&scenario.name) else {
            mismatches.push(mismatch(
                0,
                None,
                None,
                "the fixture has no result for this scenario".into(),
            ));
            continue;
        };
        let actual = match runner.run(scenario) {
            Ok(actual) => actual,
            Err(why) => {
                mismatches.push(mismatch(
                    0,
                    expected.first(),
                    None,
                    format!("the runner failed: {why}"),
                ));
                continue;
            }
        };
        for step in 0..expected.len().max(actual.len()) {
            if breaks.allows(&scenario.name, step) {
                continue;
            }
            let (want, got) = (expected.get(step), actual.get(step));
            if want == got {
                continue;
            }
            let detail = match (want, got) {
                (Some(want), Some(got)) => describe(want, got),
                (Some(_), None) => "the port stopped before this step".into(),
                (None, _) => "the port ran a step WordPress did not".into(),
            };
            mismatches.push(mismatch(step, want, got, detail));
            break;
        }
    }
    mismatches
}

fn describe(want: &StepResult, got: &StepResult) -> String {
    let pair = |w: &Value, g: &Value| format!("expected {}, got {}", to_text(w), to_text(g));
    if want.returned != got.returned {
        return format!("returned: {}", pair(&want.returned, &got.returned));
    }
    if want.invocations != got.invocations {
        let as_value = |i: &[Invocation]| serde_json::to_value(i).unwrap_or(Value::Null);
        return format!(
            "invocations: {}",
            pair(&as_value(&want.invocations), &as_value(&got.invocations))
        );
    }
    let (w, g) = (&want.globals, &got.globals);
    for (name, a, b) in [
        ("$wp_filter", &w.wp_filter, &g.wp_filter),
        ("$wp_actions", &w.wp_actions, &g.wp_actions),
        ("$wp_filters", &w.wp_filters, &g.wp_filters),
        (
            "$wp_current_filter",
            &w.wp_current_filter,
            &g.wp_current_filter,
        ),
    ] {
        if a != b {
            return format!("{name}: {}", pair(a, b));
        }
    }
    "the results differ".into()
}

fn to_text(value: &Value) -> String {
    let text = serde_json::to_string(value).unwrap_or_default();
    if text.len() > 600 {
        format!("{}…", &text[..text.floor_char_boundary(600)])
    } else {
        text
    }
}
