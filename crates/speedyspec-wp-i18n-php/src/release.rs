//! A WordPress release prepared for the native translation functions, on top of the UTF-8 port's preparation:
//! `l10n.php` without the functions declared natively, and the translation controller, file, `WP_Translations`,
//! `WP_Locale`, `WP_Locale_Switcher` and `Plural_Forms` classes replaced by stubs (the facade declares them). `wp-settings.php` still `require`s them.

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
    "translate_settings_using_i18n_schema",
];

const STUBBED: &[&str] = &[
    "class-wp-translation-controller.php",
    "class-wp-translations.php",
    "class-wp-translation-file.php",
    "class-wp-translation-file-mo.php",
    "class-wp-translation-file-php.php",
];

/// Files in `wp-includes/pomo/` that the facade declares.
const STUBBED_POMO: &[&str] = &["plural-forms.php"];

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

/// Native fast paths put at the start of WordPress's own methods, by file in `wp-includes/pomo/` and method
/// signature line. WordPress's body stays after each, for every case the fast path does not take.
const POMO_FAST_PATHS: &[(&str, &str, &str)] = &[
    (
        "mo.php",
        "\t\tpublic function import_from_file( $filename ) {\n",
        "\t\t\t// SpeedySpec: the file read natively; the pairs go to make_headers() and make_entry() as\n\t\t\t// import_from_reader() hands them over. Anything the native reader leaves (a file it cannot open, an empty\n\t\t\t// table) runs WordPress's code below.\n\t\t\tif ( \\SpeedySpec\\WpI18n\\uses_native( $this, 'import_from_reader', 'MO' ) ) {\n\t\t\t\t$pairs = \\SpeedySpec\\WpI18n\\Native\\mo_pairs( (string) $filename );\n\t\t\t\tif ( null !== $pairs ) {\n\t\t\t\t\t$this->filename = (string) $filename;\n\t\t\t\t\tif ( false === $pairs ) {\n\t\t\t\t\t\treturn false;\n\t\t\t\t\t}\n\t\t\t\t\tforeach ( $pairs as $pair ) {\n\t\t\t\t\t\tif ( '' === $pair[0] ) {\n\t\t\t\t\t\t\t$this->set_headers( $this->make_headers( $pair[1] ) );\n\t\t\t\t\t\t} else {\n\t\t\t\t\t\t\t$entry                          = &$this->make_entry( $pair[0], $pair[1] );\n\t\t\t\t\t\t\t$this->entries[ $entry->key() ] = &$entry;\n\t\t\t\t\t\t}\n\t\t\t\t\t}\n\t\t\t\t\treturn true;\n\t\t\t\t}\n\t\t\t}\n\n",
    ),
    (
        "mo.php",
        "\t\tpublic function export_to_file_handle( $fh ) {\n",
        "\t\t\t// SpeedySpec: WordPress's filtering, ordering and (overridable) per-entry strings, called as often and in the\n\t\t\t// order WordPress calls them, then one native pack and one write.\n\t\t\t$entries = array_filter( $this->entries, array( $this, 'is_entry_good_for_export' ) );\n\t\t\tksort( $entries );\n\t\t\t$originals = array();\n\t\t\tforeach ( $entries as $entry ) {\n\t\t\t\t$originals[] = $this->export_original( $entry );\n\t\t\t\t$this->export_original( $entry );\n\t\t\t}\n\t\t\t$exported_headers = $this->export_headers();\n\t\t\t$translations     = array();\n\t\t\tforeach ( $entries as $entry ) {\n\t\t\t\t$translations[] = $this->export_translations( $entry );\n\t\t\t\t$this->export_translations( $entry );\n\t\t\t}\n\t\t\t$packed = \\SpeedySpec\\WpI18n\\Native\\mo_pack( $originals, $translations, $exported_headers );\n\t\t\tif ( null !== $packed ) {\n\t\t\t\tfwrite( $fh, $packed );\n\t\t\t\treturn true;\n\t\t\t}\n\n",
    ),
    (
        "po.php",
        "\t\tpublic function import_from_file( $filename ) {\n",
        "\t\t\t// SpeedySpec: the file parsed natively when read_entry(), read_line() and add_comment_to_entry() are PO's own;\n\t\t\t// each entry becomes a Translation_Entry and goes where WordPress's loop puts it. A file the native reader\n\t\t\t// cannot open runs WordPress's code below.\n\t\t\tif ( \\SpeedySpec\\WpI18n\\uses_native( $this, 'read_entry', 'PO' ) && \\SpeedySpec\\WpI18n\\uses_native( $this, 'read_line', 'PO' )\n\t\t\t\t&& \\SpeedySpec\\WpI18n\\uses_native( $this, 'add_comment_to_entry', 'PO' )\n\t\t\t) {\n\t\t\t\t$parsed = \\SpeedySpec\\WpI18n\\Native\\po_parse( (string) $filename );\n\t\t\t\tif ( null !== $parsed ) {\n\t\t\t\t\tforeach ( $parsed[0] as $fields ) {\n\t\t\t\t\t\t$entry = new Translation_Entry();\n\t\t\t\t\t\tforeach ( $fields as $name => $value ) {\n\t\t\t\t\t\t\t$entry->$name = $value;\n\t\t\t\t\t\t}\n\t\t\t\t\t\tif ( '' === $entry->singular ) {\n\t\t\t\t\t\t\t$this->set_headers( $this->make_headers( $entry->translations[0] ) );\n\t\t\t\t\t\t} else {\n\t\t\t\t\t\t\t$this->add_entry( $entry );\n\t\t\t\t\t\t}\n\t\t\t\t\t}\n\t\t\t\t\t$this->read_line( null, 'clear' );\n\t\t\t\t\tif ( $parsed[1] ) {\n\t\t\t\t\t\treturn false;\n\t\t\t\t\t}\n\t\t\t\t\tif ( ! $this->headers && ! $this->entries ) {\n\t\t\t\t\t\treturn false;\n\t\t\t\t\t}\n\t\t\t\t\treturn true;\n\t\t\t\t}\n\t\t\t}\n\n",
    ),
    (
        "po.php",
        "\t\tpublic static function poify( $input_string ) {\n",
        "\t\t\t// SpeedySpec: strings take the native path.\n\t\t\tif ( is_string( $input_string ) ) {\n\t\t\t\treturn \\SpeedySpec\\WpI18n\\Native\\po_poify( $input_string );\n\t\t\t}\n\n",
    ),
    (
        "po.php",
        "\t\tpublic static function unpoify( $input_string ) {\n",
        "\t\t\t// SpeedySpec: strings take the native path.\n\t\t\tif ( is_string( $input_string ) ) {\n\t\t\t\treturn \\SpeedySpec\\WpI18n\\Native\\po_unpoify( $input_string );\n\t\t\t}\n\n",
    ),
    (
        "po.php",
        "\t\tpublic static function prepend_each_line( $input_string, $with ) {\n",
        "\t\t\t// SpeedySpec: strings take the native path.\n\t\t\tif ( is_string( $input_string ) && is_string( $with ) ) {\n\t\t\t\treturn \\SpeedySpec\\WpI18n\\Native\\po_prepend_each_line( $input_string, $with );\n\t\t\t}\n\n",
    ),
    (
        "po.php",
        "\t\tpublic static function comment_block( $text, $char = ' ' ) {\n",
        "\t\t\t// SpeedySpec: strings take the native path.\n\t\t\tif ( is_string( $text ) && is_string( $char ) ) {\n\t\t\t\treturn \\SpeedySpec\\WpI18n\\Native\\po_comment_block( $text, $char );\n\t\t\t}\n\n",
    ),
    (
        "po.php",
        "\t\tpublic static function match_begin_and_end_newlines( $translation, $original ) {\n",
        "\t\t\t// SpeedySpec: strings take the native path.\n\t\t\tif ( is_string( $translation ) && is_string( $original ) ) {\n\t\t\t\treturn \\SpeedySpec\\WpI18n\\Native\\po_match_newlines( $translation, $original );\n\t\t\t}\n\n",
    ),
    (
        "po.php",
        "\t\tpublic static function trim_quotes( $s ) {\n",
        "\t\t\t// SpeedySpec: strings take the native path.\n\t\t\tif ( is_string( $s ) ) {\n\t\t\t\treturn \\SpeedySpec\\WpI18n\\Native\\po_trim_quotes( $s );\n\t\t\t}\n\n",
    ),
];

/// `source` with each of `fast_paths` (for `file`) put right after its method's signature line.
pub fn patch_pomo(file: &str, source: &str) -> Result<String, String> {
    let mut out = source.to_owned();
    for (f, signature, body) in POMO_FAST_PATHS {
        if *f != file {
            continue;
        }
        let at = out
            .find(signature)
            .ok_or_else(|| format!("pomo/{file} has no `{}`", signature.trim()))?;
        out.insert_str(at + signature.len(), body);
    }
    Ok(out)
}

/// Makes `out_includes/<dir>` a real directory: `stubbed` files written as stubs, the rest linked to the release's.
fn stub_dir(
    includes: &Path,
    out_includes: &Path,
    dir: &str,
    stubbed: &[&str],
) -> Result<(), String> {
    let target_dir = out_includes.join(dir);
    if target_dir
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        std::fs::remove_file(&target_dir).map_err(|e| err(&target_dir, e))?;
    }
    std::fs::create_dir_all(&target_dir).map_err(|e| err(&target_dir, e))?;
    let source_dir = includes.join(dir);
    for entry in std::fs::read_dir(&source_dir).map_err(|e| err(&source_dir, e))? {
        let entry = entry.map_err(|e| err(&source_dir, e))?;
        let name = entry.file_name();
        let target = target_dir.join(&name);
        let file = name.to_string_lossy();
        if stubbed.contains(&file.as_ref()) {
            write_if_changed(&target, STUB)?;
        } else if dir == "pomo" && POMO_FAST_PATHS.iter().any(|(f, _, _)| *f == file) {
            let path = entry.path();
            let source = std::fs::read_to_string(&path).map_err(|e| err(&path, e))?;
            write_if_changed(&target, &patch_pomo(&file, &source)?)?;
        } else if target.symlink_metadata().is_err() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(entry.path(), &target).map_err(|e| err(&target, e))?;
        }
    }
    Ok(())
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
    stub_dir(&includes, &out_includes, "l10n", STUBBED)?;
    stub_dir(&includes, &out_includes, "pomo", STUBBED_POMO)?;
    Ok(out.to_path_buf())
}
