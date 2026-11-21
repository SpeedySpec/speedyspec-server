//! A WordPress release prepared for the native translation functions, on top of the UTF-8 port's preparation:
//! `l10n.php` without the functions declared natively, and the translation controller, file, `WP_Translations`,
//! `WP_Locale` and `WP_Locale_Switcher` classes replaced by stubs (the facade declares them). `wp-settings.php` still `require`s them.

use std::path::{Path, PathBuf};

/// The `l10n.php` functions declared natively.
pub const PORTED_FUNCTIONS: &[&str] = &[
    "translate",
    "translate_with_gettext_context",
    "__",
    "_x",
    "_n",
    "_nx",
    "_load_script_textdomain_from_src",
    "load_script_translations",
];

const STUBBED: &[&str] = &[
    "class-wp-translation-controller.php",
    "class-wp-translations.php",
    "class-wp-translation-file.php",
    "class-wp-translation-file-mo.php",
    "class-wp-translation-file-php.php",
];

/// Files in `wp-includes/` itself that the facade declares.
const STUBBED_INCLUDES: &[&str] = &["class-wp-locale.php", "class-wp-locale-switcher.php"];

const STUB: &str = "<?php\n// Provided by speedyspec-wp-i18n-php.\n";

/// `l10n.php`'s text without the ported functions, each with its docblock.
pub fn strip_ported(source: &str) -> Result<String, String> {
    let lines: Vec<&str> = source.split_inclusive('\n').collect();
    let mut keep = vec![true; lines.len()];
    for name in PORTED_FUNCTIONS {
        let start = lines
            .iter()
            .position(|l| l.starts_with(&format!("function {name}(")))
            .ok_or_else(|| format!("l10n.php has no function {name}()"))?;
        let end = (start..lines.len())
            .find(|&i| lines[i].trim_end() == "}")
            .ok_or_else(|| format!("l10n.php's {name}() has no end"))?;
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

fn err(p: &Path, e: std::io::Error) -> String {
    format!("{}: {e}", p.display())
}

fn write_if_changed(path: &Path, text: &str) -> Result<(), String> {
    if path
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        std::fs::remove_file(path).map_err(|e| err(path, e))?;
    }
    if std::fs::read_to_string(path).ok().as_deref() == Some(text) {
        return Ok(());
    }
    let temp = path.with_extension(format!("php.{}", std::process::id()));
    std::fs::write(&temp, text).map_err(|e| err(&temp, e))?;
    std::fs::rename(&temp, path).map_err(|e| err(path, e))
}

/// Prepares `release` in `out` for both the UTF-8 and the translation natives. Returns `out`.
pub fn prepare_release(release: &Path, out: &Path) -> Result<PathBuf, String> {
    speedyspec_wp_utf8_php::prepare_release(release, out)?;
    let includes = release.join("wp-includes");
    let out_includes = out.join("wp-includes");
    let l10n = includes.join("l10n.php");
    let stripped = strip_ported(&std::fs::read_to_string(&l10n).map_err(|e| err(&l10n, e))?)?;
    write_if_changed(&out_includes.join("l10n.php"), &stripped)?;
    for name in STUBBED_INCLUDES {
        write_if_changed(&out_includes.join(name), STUB)?;
    }
    let dir = out_includes.join("l10n");
    if dir
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        std::fs::remove_file(&dir).map_err(|e| err(&dir, e))?;
    }
    std::fs::create_dir_all(&dir).map_err(|e| err(&dir, e))?;
    let source_dir = includes.join("l10n");
    for entry in std::fs::read_dir(&source_dir).map_err(|e| err(&source_dir, e))? {
        let entry = entry.map_err(|e| err(&source_dir, e))?;
        let name = entry.file_name();
        let target = dir.join(&name);
        if STUBBED.contains(&name.to_string_lossy().as_ref()) {
            write_if_changed(&target, STUB)?;
        } else if target.symlink_metadata().is_err() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(entry.path(), &target).map_err(|e| err(&target, e))?;
        }
    }
    Ok(out.to_path_buf())
}
