//! A WordPress release prepared for the native cron functions: the options port's prepared release, with
//! `wp-includes/cron.php` written without the functions this port declares (which would otherwise be declared twice).

use std::path::{Path, PathBuf};

use speedyspec_compat::ReleaseMirror;

/// The `cron.php` functions the native area declares: all of them.
pub const PORTED_FUNCTIONS: &[&str] = &[
    "wp_schedule_single_event",
    "wp_schedule_event",
    "wp_reschedule_event",
    "wp_unschedule_event",
    "wp_clear_scheduled_hook",
    "wp_unschedule_hook",
    "wp_get_scheduled_event",
    "wp_next_scheduled",
    "wp_get_schedules",
    "wp_get_schedule",
    "wp_get_ready_cron_jobs",
    "_get_cron_array",
    "_set_cron_array",
    "_upgrade_cron_array",
    "spawn_cron",
    "wp_cron",
    "_wp_cron",
];

/// `cron.php`'s text without the ported functions, each with its docblock. A function is a line
/// `function <name>(` at the start of a line through the next line that is `}` alone, as WordPress formats them.
pub fn strip_ported(source: &str) -> Result<String, String> {
    let lines: Vec<&str> = source.split_inclusive('\n').collect();
    let mut keep = vec![true; lines.len()];
    for name in PORTED_FUNCTIONS {
        let start = lines
            .iter()
            .position(|l| l.starts_with(&format!("function {name}(")))
            .ok_or_else(|| format!("cron.php has no function {name}()"))?;
        let end = (start..lines.len())
            .find(|&i| lines[i].trim_end() == "}")
            .ok_or_else(|| format!("cron.php's {name}() has no end"))?;
        let mut first = start;
        if first > 0 && lines[first - 1].trim_end() == " */" {
            while first > 0 && !lines[first - 1].starts_with("/**") {
                first -= 1;
            }
            first = first.saturating_sub(1);
        }
        keep[first..=end].iter_mut().for_each(|k| *k = false);
    }
    Ok(lines
        .iter()
        .zip(keep)
        .filter(|(_, k)| *k)
        .map(|(l, _)| *l)
        .collect())
}

/// Prepares `release` (a `wordpress/` root) in `out`: [`speedyspec_wp_options_php::prepare_release`]'s copy, with
/// `wp-includes/cron.php` (a link there) replaced by a copy without the ported functions. Returns `out`.
pub fn prepare_release(release: &Path, out: &Path) -> Result<PathBuf, String> {
    speedyspec_wp_options_php::prepare_release(release, out)?;
    let mirror = ReleaseMirror::new(release, out)?;
    let cron = "wp-includes/cron.php";
    mirror.write(cron, &strip_ported(&mirror.read(cron)?)?)?;
    Ok(out.to_path_buf())
}
