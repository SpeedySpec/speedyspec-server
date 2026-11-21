//! A WordPress release prepared for the native kses functions: its `kses.php` and `functions.php` without the
//! functions this port declares (which would otherwise be declared twice), every other file the release's own.

use std::path::{Path, PathBuf};

/// The functions the native area declares: every function of 7.1.5's `kses.php` but
/// `_wp_kses_sanitize_note_mention_classes` (it needs the HTML API's tag processor) and
/// `wp_filter_global_styles_post` (it needs the theme JSON port), plus `wp_allowed_protocols` from `functions.php`.
pub const PORTED_FUNCTIONS: &[&str] = &[
    "wp_kses",
    "wp_kses_one_attr",
    "wp_kses_allowed_html",
    "_wp_kses_allow_note_mention_span",
    "wp_kses_hook",
    "wp_kses_version",
    "wp_kses_split",
    "wp_kses_uri_attributes",
    "_wp_kses_split_callback",
    "wp_kses_split2",
    "wp_kses_attr",
    "wp_kses_attr_check",
    "wp_kses_hair",
    "wp_kses_attr_parse",
    "wp_kses_hair_parse",
    "wp_kses_check_attr_val",
    "wp_kses_bad_protocol",
    "wp_kses_no_null",
    "wp_kses_stripslashes",
    "wp_kses_array_lc",
    "wp_kses_html_error",
    "wp_kses_bad_protocol_once",
    "wp_kses_bad_protocol_once2",
    "wp_kses_normalize_entities",
    "wp_kses_named_entities",
    "wp_kses_xml_named_entities",
    "wp_kses_normalize_entities2",
    "wp_kses_normalize_entities3",
    "valid_unicode",
    "wp_kses_decode_entities",
    "_wp_kses_decode_entities_chr",
    "_wp_kses_decode_entities_chr_hexdec",
    "wp_filter_kses",
    "wp_kses_data",
    "wp_filter_post_kses",
    "wp_kses_post",
    "wp_kses_post_deep",
    "wp_filter_nohtml_kses",
    "kses_init_filters",
    "kses_remove_filters",
    "kses_init",
    "safecss_filter_attr",
    "_wp_add_global_attributes",
    "_wp_kses_allow_pdf_objects",
    "_wp_kses_restricts_attribute_values",
    "_wp_kses_directive_binds_value_restricted_attribute",
    "wp_allowed_protocols",
];

/// A release file the port strips, and which of [`PORTED_FUNCTIONS`] it declares.
type StrippedFile = (&'static str, fn(&str) -> bool);

/// The release files the port strips.
const FILES: &[StrippedFile] = &[
    ("kses.php", |name| name != "wp_allowed_protocols"),
    ("functions.php", |name| name == "wp_allowed_protocols"),
];

/// `file`'s text (`kses.php` or `functions.php` of `wp-includes/`) without the ported functions it declares, each
/// with its docblock. A function is a line `function <name>(` at the start of a line through the next line that is
/// `}` alone, as WordPress formats them.
pub fn strip_ported(file: &str, source: &str) -> Result<String, String> {
    let (_, declares) = FILES
        .iter()
        .find(|(f, _)| *f == file)
        .ok_or_else(|| format!("the kses port strips no {file}"))?;
    let lines: Vec<&str> = source.split_inclusive('\n').collect();
    let mut keep = vec![true; lines.len()];
    for name in PORTED_FUNCTIONS.iter().filter(|n| declares(n)) {
        let start = lines
            .iter()
            .position(|l| l.starts_with(&format!("function {name}(")))
            .ok_or_else(|| format!("{file} has no function {name}()"))?;
        let end = (start..lines.len())
            .find(|&i| lines[i].trim_end() == "}")
            .ok_or_else(|| format!("{file}'s {name}() has no end"))?;
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

/// Prepares `release` (a `wordpress/` root) in `out`: links to each of its entries, except the files of [`FILES`],
/// which are written without the ported functions. Reuses a copy already prepared, rewriting a stripped file that
/// differs (another build's port list). Returns `out`.
pub fn prepare_release(release: &Path, out: &Path) -> Result<PathBuf, String> {
    let err = |p: &Path, e: std::io::Error| format!("{}: {e}", p.display());
    let includes = release.join("wp-includes");
    std::fs::create_dir_all(out.join("wp-includes")).map_err(|e| err(out, e))?;
    for entry in std::fs::read_dir(release).map_err(|e| err(release, e))? {
        let entry = entry.map_err(|e| err(release, e))?;
        if entry.file_name() != "wp-includes" {
            link(&entry.path(), &out.join(entry.file_name()))?;
        }
    }
    for entry in std::fs::read_dir(&includes).map_err(|e| err(&includes, e))? {
        let entry = entry.map_err(|e| err(&includes, e))?;
        let stripped = FILES.iter().any(|(f, _)| entry.file_name() == *f);
        if !stripped {
            link(
                &entry.path(),
                &out.join("wp-includes").join(entry.file_name()),
            )?;
        }
    }
    for (file, _) in FILES {
        let source = includes.join(file);
        let wanted = strip_ported(
            file,
            &std::fs::read_to_string(&source).map_err(|e| err(&source, e))?,
        )?;
        let target = out.join("wp-includes").join(file);
        if std::fs::read_to_string(&target).ok().as_deref() != Some(wanted.as_str()) {
            let staged = target.with_extension(format!("php.{}", std::process::id()));
            std::fs::write(&staged, wanted).map_err(|e| err(&staged, e))?;
            std::fs::rename(&staged, &target).map_err(|e| err(&target, e))?;
        }
    }
    Ok(out.to_path_buf())
}

fn link(from: &Path, to: &Path) -> Result<(), String> {
    if to.exists() || to.symlink_metadata().is_ok() {
        return Ok(());
    }
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(from, to);
    #[cfg(not(unix))]
    let made = if from.is_dir() {
        Err(std::io::Error::other("links need unix"))
    } else {
        std::fs::copy(from, to).map(|_| ())
    };
    made.map_err(|e| format!("{}: {e}", to.display()))
}
