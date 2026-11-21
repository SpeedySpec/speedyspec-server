//! Script translations: where `_load_script_textdomain_from_src()` looks for a script's JSON file, and
//! `load_script_translations()`, with the files read once per worker (Identity Map, as translation files are).

use std::path::Path;
use std::rc::Rc;

use md5::{Digest, Md5};
use speedyspec_wp_i18n_contracts::{
    FileProbe, PhpValue, ScriptSources, TranslationFileCache, UrlParts, php_truthy,
};

use crate::{I18nApi, I18nError, bytes_value};

/// Where a script's source sits relative to WordPress, as `_load_script_textdomain_from_src()` works it out before
/// its `load_script_textdomain_relative_path` filter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Relative {
    /// `wp_parse_url()` could not parse the source or `content_url()`: no file is looked for.
    Unparseable,
    /// The path relative to the plugin, theme or WordPress root (`None` is WordPress's false: not WordPress's
    /// script), and the language subdirectory (`plugins` or `themes`) to look in, if any.
    Path {
        relative: Option<String>,
        languages_suffix: Option<&'static str>,
    },
}

fn trim_slashes(s: &str) -> &str {
    s.trim_matches('/')
}

fn untrailingslashit(s: &str) -> &str {
    s.trim_end_matches(['/', '\\'])
}

/// Whether `url` is on the same host as `base`, as WordPress compares them (a side without a host matches).
fn same_host(url: &UrlParts, base: &UrlParts) -> bool {
    match (&url.host, &base.host) {
        (Some(a), Some(b)) => a == b,
        _ => true,
    }
}

/// The source path below `base`'s path (all of it when `base` has none), when it starts there and is on its host.
fn below<'a>(src: &'a UrlParts, src_path: &'a str, base: &UrlParts) -> Option<&'a str> {
    let starts = match &base.path {
        None => true,
        Some(p) => src_path.starts_with(p.as_str()),
    };
    if !(starts && same_host(src, base)) {
        return None;
    }
    Some(match &base.path {
        Some(p) => &src_path[p.len()..],
        None => src_path,
    })
}

/// The relative path and language subdirectory for `src`, given the parsed `content_url()`, `plugins_url()` and
/// `site_url()` (each `None` when `wp_parse_url()` returned false) and `get_theme_root()`.
pub fn relative_path(
    src: &UrlParts,
    content: Option<&UrlParts>,
    plugins: Option<&UrlParts>,
    site: Option<&UrlParts>,
    theme_root: &str,
) -> Relative {
    let Some(content) = content else {
        return Relative::Unparseable;
    };
    let src_path = src.path.as_deref().unwrap_or("");
    if let Some(rest) = below(src, src_path, content) {
        let parts: Vec<&str> = trim_slashes(rest).split('/').collect();
        let theme_dir = theme_root.rsplit('/').next().unwrap_or("");
        let suffix = if parts[0] == theme_dir {
            "themes"
        } else {
            "plugins"
        };
        return Relative::Path {
            relative: Some(parts.iter().skip(2).copied().collect::<Vec<_>>().join("/")),
            languages_suffix: Some(suffix),
        };
    }
    let plugins = plugins.cloned().unwrap_or_default();
    if let Some(rest) = below(src, src_path, &plugins) {
        let parts: Vec<&str> = trim_slashes(rest).split('/').collect();
        return Relative::Path {
            relative: Some(parts.iter().skip(1).copied().collect::<Vec<_>>().join("/")),
            languages_suffix: Some("plugins"),
        };
    }
    let site = site.cloned().unwrap_or_default();
    let mut relative = None;
    if same_host(src, &site) {
        match &site.path {
            None => relative = Some(trim_slashes(src_path).to_owned()),
            Some(p) => {
                let with_slash = format!("{}/", untrailingslashit(p));
                if src_path.starts_with(&with_slash) {
                    relative = Some(trim_slashes(&src_path[p.len()..]).to_owned());
                }
            }
        }
    }
    Relative::Path {
        relative,
        languages_suffix: None,
    }
}

/// The JSON file name for a script at `relative`: translations are always for the unminified file.
pub fn json_filename(file_base: &str, relative: &str) -> String {
    let relative = match relative.strip_suffix(".min.js") {
        Some(stem) => format!("{stem}.js"),
        None => relative.to_owned(),
    };
    let digest = Md5::digest(relative.as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("{file_base}-{hex}.json")
}

/// Script translation files, read once per worker and kept while their identity is unchanged.
pub struct ScriptFiles {
    probe: Rc<dyn FileProbe>,
    cache: Rc<dyn TranslationFileCache<Vec<u8>>>,
}

impl ScriptFiles {
    pub fn new(
        probe: Rc<dyn FileProbe>,
        cache: Rc<dyn TranslationFileCache<Vec<u8>>>,
    ) -> ScriptFiles {
        ScriptFiles { probe, cache }
    }

    /// `is_readable()`.
    pub fn is_readable(&self, path: &Path) -> bool {
        self.probe.is_readable(path)
    }

    /// The file's bytes (`file_get_contents()`), from the cache when the file is unchanged; `None` when it cannot
    /// be read.
    pub fn read(&self, path: &Path) -> Option<Rc<Vec<u8>>> {
        let identity = self.probe.identity(path)?;
        if let Some(bytes) = self.cache.get(&identity) {
            return Some(bytes);
        }
        let bytes = Rc::new(std::fs::read(path).ok()?);
        self.cache.put(identity, bytes.clone());
        Some(bytes)
    }
}

/// A value PHP's `is_readable()` would take as a file name, or its `TypeError`.
fn file_name(file: &PhpValue) -> Result<Vec<u8>, I18nError> {
    Ok(match file {
        PhpValue::Str(s) => s.as_bytes().to_vec(),
        PhpValue::Bytes(b) => b.clone(),
        PhpValue::Int(n) => n.to_string().into_bytes(),
        PhpValue::Float(f) => f.to_string().into_bytes(),
        PhpValue::Bool(true) => b"1".to_vec(),
        PhpValue::Null | PhpValue::Bool(false) => Vec::new(),
        PhpValue::Array(_) => {
            return Err(I18nError::Type(
                "is_readable(): Argument #1 ($filename) must be of type string, array given".into(),
            ));
        }
        PhpValue::Opaque(_) => {
            return Err(I18nError::Type(
                "is_readable(): Argument #1 ($filename) must be of type string, object given"
                    .into(),
            ));
        }
    })
}

fn port(e: speedyspec_wp_i18n_contracts::PortError) -> I18nError {
    I18nError::Port(e)
}

impl I18nApi {
    /// `load_script_translations( $file, $handle, $domain )`: the JSON text, `false`, or whatever its filters
    /// return.
    pub fn load_script_translations(
        &self,
        files: &ScriptFiles,
        file: PhpValue,
        handle: &str,
        domain: &str,
    ) -> Result<PhpValue, I18nError> {
        let rest = |f: &PhpValue| {
            vec![
                f.clone(),
                PhpValue::Str(handle.to_owned()),
                PhpValue::Str(domain.to_owned()),
            ]
        };
        let pre = self.filter("pre_load_script_translations", PhpValue::Null, rest(&file))?;
        if pre != PhpValue::Null {
            return Ok(pre);
        }
        let file = self.filter(
            "load_script_translation_file",
            file.clone(),
            vec![
                PhpValue::Str(handle.to_owned()),
                PhpValue::Str(domain.to_owned()),
            ],
        )?;
        if !php_truthy(&file) {
            return Ok(PhpValue::Bool(false));
        }
        let name = file_name(&file)?;
        let path = Path::new(std::ffi::OsStr::from_bytes_lossless(&name));
        if !files.is_readable(path) {
            return Ok(PhpValue::Bool(false));
        }
        let translations = match files.read(path) {
            Some(bytes) => bytes_value(&bytes),
            None => PhpValue::Bool(false),
        };
        self.filter("load_script_translations", translations, rest(&file))
    }

    /// `_load_script_textdomain_from_src( $handle, $src, $domain, $path, $is_module )`.
    #[allow(clippy::too_many_arguments)]
    pub fn load_script_textdomain_from_src(
        &self,
        sources: &dyn ScriptSources,
        files: &ScriptFiles,
        handle: &str,
        src: &str,
        domain: &str,
        path: &str,
        is_module: bool,
    ) -> Result<PhpValue, I18nError> {
        let locale = sources.determine_locale().map_err(port)?;
        let mut path = path.to_owned();
        if !php_truthy(&PhpValue::Str(path.clone())) {
            path = sources
                .registry_path(domain, &locale)
                .map_err(port)?
                .unwrap_or_default();
        }
        let has_path = php_truthy(&PhpValue::Str(path.clone()));
        if has_path {
            path = untrailingslashit(&path).to_owned();
        }
        let file_base = if domain == "default" {
            locale.clone()
        } else {
            format!("{domain}-{locale}")
        };
        let at = |dir: &str, name: &str| PhpValue::Str(format!("{dir}/{name}"));
        if has_path {
            let t = self.load_script_translations(
                files,
                at(&path, &format!("{file_base}-{handle}.json")),
                handle,
                domain,
            )?;
            if php_truthy(&t) {
                return Ok(t);
            }
        }
        let none = |api: &I18nApi| {
            api.load_script_translations(files, PhpValue::Bool(false), handle, domain)
        };
        let Some(src_url) = sources.parse_url(src).map_err(port)? else {
            return none(self);
        };
        let Some(content) = sources
            .parse_url(&sources.content_url().map_err(port)?)
            .map_err(port)?
        else {
            return none(self);
        };
        let plugins = sources
            .parse_url(&sources.plugins_url().map_err(port)?)
            .map_err(port)?;
        let site = sources
            .parse_url(&sources.site_url().map_err(port)?)
            .map_err(port)?;
        let theme_root = sources.theme_root().map_err(port)?;
        let (relative, suffix) = match relative_path(
            &src_url,
            Some(&content),
            plugins.as_ref(),
            site.as_ref(),
            &theme_root,
        ) {
            Relative::Unparseable => return none(self),
            Relative::Path {
                relative,
                languages_suffix,
            } => (relative, languages_suffix),
        };
        let relative = self.filter(
            "load_script_textdomain_relative_path",
            relative.map(PhpValue::Str).unwrap_or(PhpValue::Bool(false)),
            vec![PhpValue::Str(src.to_owned()), PhpValue::Bool(is_module)],
        )?;
        let relative = match &relative {
            PhpValue::Str(s) => s.clone(),
            PhpValue::Bytes(b) => String::from_utf8_lossy(b).into_owned(),
            _ => return none(self),
        };
        let md5_filename = json_filename(&file_base, &relative);
        if has_path {
            let t =
                self.load_script_translations(files, at(&path, &md5_filename), handle, domain)?;
            if php_truthy(&t) {
                return Ok(t);
            }
        }
        let languages_path = match suffix {
            Some(sub) => format!("{}/{sub}", sources.lang_dir()),
            None => sources.lang_dir(),
        };
        let t = self.load_script_translations(
            files,
            at(&languages_path, &md5_filename),
            handle,
            domain,
        )?;
        if php_truthy(&t) {
            return Ok(t);
        }
        none(self)
    }
}

/// `OsStr` from bytes: on Unix any bytes are a path.
trait FromBytesLossless {
    fn from_bytes_lossless(b: &[u8]) -> &std::ffi::OsStr;
}

impl FromBytesLossless for std::ffi::OsStr {
    #[cfg(unix)]
    fn from_bytes_lossless(b: &[u8]) -> &std::ffi::OsStr {
        std::os::unix::ffi::OsStrExt::from_bytes(b)
    }
}
