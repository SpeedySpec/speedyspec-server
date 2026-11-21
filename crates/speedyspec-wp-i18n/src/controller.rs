//! `WP_Translation_Controller`: which translation files are loaded for which locale and text domain, and lookups
//! across them. Its state is the request's (Repository over the loaded translations); the files themselves come
//! from the per-worker cache and are shared.

use std::cell::Cell;
use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use indexmap::IndexMap as StdIndexMap;

/// Ordered maps with a fast hasher: lookups by locale and domain run on every translation call.
type IndexMap<K, V> = StdIndexMap<K, V, foldhash::fast::RandomState>;
use speedyspec_wp_i18n_contracts::{
    FileKind, FileProbe, RawTranslations, ReadError, TranslationFileCache, TranslationFileReader,
};

use crate::file::TranslationFile;
use crate::plural::PluralError;

/// A PHP array key: a string that is a canonical decimal integer becomes an integer key.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PhpKey {
    Int(i64),
    Str(Vec<u8>),
}

/// The array key PHP makes of `bytes`.
pub fn php_key(bytes: &[u8]) -> PhpKey {
    let canonical = match bytes {
        b"0" => true,
        [b'-', rest @ ..] => {
            matches!(rest.first(), Some(b'1'..=b'9')) && rest.iter().all(u8::is_ascii_digit)
        }
        [first, rest @ ..] => (b'1'..=b'9').contains(first) && rest.iter().all(u8::is_ascii_digit),
        [] => false,
    };
    if canonical && let Ok(n) = std::str::from_utf8(bytes).unwrap_or("").parse::<i64>() {
        return PhpKey::Int(n);
    }
    PhpKey::Str(bytes.to_vec())
}

/// One `WP_Translation_File` object of this request: a shared file plus whether it has been parsed, which in
/// WordPress happens on first use, so an error only shows after a lookup.
#[derive(Debug)]
pub struct FileHandle {
    file: Rc<TranslationFile>,
    path: PathBuf,
    parsed: Cell<bool>,
}

impl FileHandle {
    pub fn file(&self) -> &Rc<TranslationFile> {
        &self.file
    }

    /// `get_file()`: the real path the file was loaded from.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// `error()`: nothing until the file has been used.
    pub fn error(&self) -> Option<&ReadError> {
        if self.parsed.get() {
            self.file.error()
        } else {
            None
        }
    }

    /// Marks the file as parsed and hands it out, as every reading method of `WP_Translation_File` does.
    pub fn parse(&self) -> &TranslationFile {
        self.parsed.set(true);
        &self.file
    }
}

impl std::fmt::Debug for TranslationFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TranslationFile")
            .field("identity", self.identity())
            .finish_non_exhaustive()
    }
}

/// The loaded file for `path` from `cache`, reading it with `reader` on a miss. A reader may run PHP (the
/// `.l10n.php` fallback), which may translate, so callers holding a controller call this before borrowing it.
pub fn cached_file(
    reader: &dyn TranslationFileReader,
    probe: &dyn FileProbe,
    cache: &dyn TranslationFileCache<TranslationFile>,
    path: &Path,
) -> Option<Rc<TranslationFile>> {
    let kind = FileKind::of_path(path)?;
    cached_file_as(reader, probe, cache, path, kind)
}

/// As [`cached_file`], reading `path` as `kind` whatever it is called. A file read as another kind than its name
/// says (a temporary file named by `tempnam()`) is read fresh, not cached: the cache holds one parse per file.
pub fn cached_file_as(
    reader: &dyn TranslationFileReader,
    probe: &dyn FileProbe,
    cache: &dyn TranslationFileCache<TranslationFile>,
    path: &Path,
    kind: FileKind,
) -> Option<Rc<TranslationFile>> {
    let identity = probe.identity(path)?;
    let cacheable = FileKind::of_path(path) == Some(kind);
    if cacheable && let Some(file) = cache.get(&identity) {
        return Some(file);
    }
    let file = Rc::new(match reader.read_as(path, kind) {
        Ok(raw) => TranslationFile::new(identity.clone(), raw),
        // file_get_contents() failing leaves no error and no entries.
        Err(ReadError::Unreadable) => {
            TranslationFile::new(identity.clone(), RawTranslations::default())
        }
        // A PHP file this reader can't parse is, without PHP, as unusable as an invalid one.
        Err(ReadError::Unsupported) => {
            TranslationFile::failed(identity.clone(), ReadError::Invalid("Invalid data"))
        }
        Err(e) => TranslationFile::failed(identity.clone(), e),
    });
    if cacheable {
        cache.put(identity, file.clone());
    }
    Some(file)
}

/// What `unload_file()` takes: a path, or a file object.
pub enum FileRef<'a> {
    Path(&'a Path),
    Handle(&'a Rc<FileHandle>),
}

type Domains = IndexMap<String, Vec<Rc<FileHandle>>>;
/// `loaded_files`: real path, then locale, then text domain; `None` where WordPress stores `false`.
type LoadedFiles = HashMap<
    PathBuf,
    IndexMap<String, IndexMap<String, Option<Rc<FileHandle>>>>,
    foldhash::fast::RandomState,
>;

/// `WP_Translation_Controller`.
pub struct TranslationController {
    reader: Rc<dyn TranslationFileReader>,
    probe: Rc<dyn FileProbe>,
    cache: Rc<dyn TranslationFileCache<TranslationFile>>,
    current_locale: String,
    loaded_translations: IndexMap<String, Domains>,
    loaded_files: LoadedFiles,
    key: Vec<u8>,
    found: Option<(Rc<FileHandle>, Range<u32>)>,
}

impl TranslationController {
    pub fn new(
        reader: Rc<dyn TranslationFileReader>,
        probe: Rc<dyn FileProbe>,
        cache: Rc<dyn TranslationFileCache<TranslationFile>>,
    ) -> TranslationController {
        TranslationController {
            reader,
            probe,
            cache,
            current_locale: "en_US".to_owned(),
            loaded_translations: IndexMap::default(),
            loaded_files: HashMap::default(),
            key: Vec::with_capacity(128),
            found: None,
        }
    }

    /// Forgets what this request loaded; the per-worker cache keeps the files.
    pub fn reset(&mut self) {
        self.current_locale = "en_US".to_owned();
        self.loaded_translations.clear();
        self.loaded_files.clear();
        self.found = None;
    }

    pub fn locale(&self) -> &str {
        &self.current_locale
    }

    pub fn set_locale(&mut self, locale: &str) {
        self.current_locale = locale.to_owned();
    }

    /// `WP_Translation_File::create()`: a file object, or `None` when the file is unreadable or not `.mo`/`.php`.
    fn create(&self, real: &Path) -> Option<Rc<FileHandle>> {
        self.create_as(real, FileKind::of_path(real)?)
    }

    fn create_as(&self, real: &Path, kind: FileKind) -> Option<Rc<FileHandle>> {
        if !self.probe.is_readable(real) {
            return None;
        }
        let file = cached_file_as(&*self.reader, &*self.probe, &*self.cache, real, kind)?;
        Some(Rc::new(FileHandle {
            file,
            path: real.to_path_buf(),
            parsed: Cell::new(false),
        }))
    }

    /// `WP_Translation_File::create()` for code outside the controller: a file object for `path`, not parsed yet.
    pub fn open(&self, path: &Path) -> Option<Rc<FileHandle>> {
        self.create(path)
    }

    /// `WP_Translation_File::create( $file, $filetype )`: a file object for `path` read as `kind`, whatever its
    /// name.
    pub fn open_as(&self, path: &Path, kind: FileKind) -> Option<Rc<FileHandle>> {
        self.create_as(path, kind)
    }

    /// `load_file()`.
    pub fn load_file(&mut self, file: &Path, domain: &str, locale: Option<&str>) -> bool {
        let locale = locale.unwrap_or(&self.current_locale).to_owned();
        let Some(real) = self.probe.realpath(file) else {
            return false;
        };
        if let Some(Some(h)) = self
            .loaded_files
            .get(&real)
            .and_then(|l| l.get(&locale))
            .and_then(|d| d.get(domain))
        {
            return h.error().is_none();
        }
        let shared = self
            .loaded_files
            .get(&real)
            .and_then(|l| l.get(&locale))
            .filter(|d| !d.is_empty());
        let moe = match shared {
            Some(domains) => domains.values().next().cloned().flatten(),
            None => self.create(&real),
        };
        self.loaded_files
            .entry(real)
            .or_default()
            .entry(locale.clone())
            .or_default()
            .insert(domain.to_owned(), moe.clone());
        let Some(moe) = moe else {
            return false;
        };
        self.loaded_translations
            .entry(locale)
            .or_default()
            .entry(domain.to_owned())
            .or_default()
            .push(moe);
        true
    }

    fn matches(file: &FileRef<'_>, real: &Option<PathBuf>, moe: &Rc<FileHandle>) -> bool {
        match file {
            FileRef::Handle(h) => Rc::ptr_eq(h, moe),
            FileRef::Path(_) => real.as_deref() == Some(moe.path()),
        }
    }

    fn forget_loaded_file(&mut self, moe: &FileHandle, locale: &str, domain: &str) {
        if let Some(d) = self
            .loaded_files
            .get_mut(moe.path())
            .and_then(|l| l.get_mut(locale))
        {
            d.shift_remove(domain);
        }
    }

    /// `unload_file()`.
    pub fn unload_file(&mut self, file: FileRef<'_>, domain: &str, locale: Option<&str>) -> bool {
        let real = match &file {
            FileRef::Path(p) => self.probe.realpath(p),
            FileRef::Handle(_) => None,
        };
        let locales: Vec<String> = match locale {
            Some(l) => vec![l.to_owned()],
            None => self.loaded_translations.keys().cloned().collect(),
        };
        for l in &locales {
            let Some(list) = self
                .loaded_translations
                .get_mut(l)
                .and_then(|d| d.get_mut(domain))
            else {
                continue;
            };
            if let Some(i) = list.iter().position(|moe| Self::matches(&file, &real, moe)) {
                let moe = list.remove(i);
                self.forget_loaded_file(&moe, l, domain);
                return true;
            }
        }
        // With a locale, WordPress answers true even when nothing matched.
        locale.is_some()
    }

    /// `unload_textdomain()`.
    pub fn unload_textdomain(&mut self, domain: &str, locale: Option<&str>) -> bool {
        let locales: Vec<String> = match locale {
            Some(l) => vec![l.to_owned()],
            None => self.loaded_translations.keys().cloned().collect(),
        };
        let mut unloaded = false;
        for l in &locales {
            let Some(list) = self
                .loaded_translations
                .get_mut(l)
                .and_then(|d| d.shift_remove(domain))
            else {
                continue;
            };
            unloaded = true;
            for moe in &list {
                self.forget_loaded_file(moe, l, domain);
            }
        }
        unloaded
    }

    /// `is_textdomain_loaded()`.
    pub fn is_textdomain_loaded(&self, domain: &str, locale: Option<&str>) -> bool {
        let locale = locale.unwrap_or(&self.current_locale);
        self.loaded_translations
            .get(locale)
            .and_then(|d| d.get(domain))
            .is_some_and(|l| !l.is_empty())
    }

    fn files(&self, domain: &str, locale: Option<&str>) -> Vec<Rc<FileHandle>> {
        let locale = locale.unwrap_or(&self.current_locale);
        self.loaded_translations
            .get(locale)
            .and_then(|d| d.get(domain))
            .cloned()
            .unwrap_or_default()
    }

    /// `locate_translation()` for the key in `self.key`: the first loaded file with it wins; a file found broken on
    /// the way is unloaded.
    fn locate(&mut self, domain: &str, locale: Option<&str>) -> bool {
        self.found = None;
        if self.loaded_translations.is_empty() {
            return false;
        }
        let locale_key = locale.unwrap_or(&self.current_locale);
        let Some(list) = self
            .loaded_translations
            .get(locale_key)
            .and_then(|d| d.get(domain))
        else {
            return false;
        };
        // Fast path: nothing to unload when the first file has the key.
        if let Some(first) = list.first()
            && let Some(range) = first.parse().find(&self.key)
        {
            self.found = Some((first.clone(), range));
            return true;
        }
        for moe in self.files(domain, locale) {
            if let Some(range) = moe.parse().find(&self.key) {
                self.found = Some((moe, range));
                return true;
            }
            if moe.error().is_some() {
                self.unload_file(FileRef::Handle(&moe), domain, locale);
            }
        }
        false
    }

    fn set_key(&mut self, context: &[u8], text: &[&[u8]]) {
        self.key.clear();
        if !context.is_empty() {
            self.key.extend_from_slice(context);
            self.key.push(4);
        }
        for (i, part) in text.iter().enumerate() {
            if i > 0 {
                self.key.push(0);
            }
            self.key.extend_from_slice(part);
        }
    }

    fn found_bytes(&self) -> &[u8] {
        let (moe, range) = self.found.as_ref().expect("locate() found it");
        moe.file().at(range)
    }

    /// `translate()`: the first form, or `None`.
    pub fn translate(
        &mut self,
        text: &[u8],
        context: &[u8],
        domain: &str,
        locale: Option<&str>,
    ) -> Option<&[u8]> {
        self.set_key(context, &[text]);
        if !self.locate(domain, locale) {
            return None;
        }
        let v = self.found_bytes();
        Some(v.split(|b| *b == 0).next().unwrap_or(v))
    }

    /// `translate_plural()`: the joined key first, then the singular; the form the file's rule picks, falling back
    /// to the first.
    pub fn translate_plural(
        &mut self,
        singular: &[u8],
        plural: &[u8],
        n: i64,
        context: &[u8],
        domain: &str,
        locale: Option<&str>,
    ) -> Result<Option<&[u8]>, PluralError> {
        self.translate_plurals(&[singular, plural], n, context, domain, locale)
    }

    /// `translate_plural()` for any number of originals, as WordPress joins them all with `\0` for the first
    /// lookup and falls back to the first alone.
    pub fn translate_plurals(
        &mut self,
        plurals: &[&[u8]],
        n: i64,
        context: &[u8],
        domain: &str,
        locale: Option<&str>,
    ) -> Result<Option<&[u8]>, PluralError> {
        let Some(first_original) = plurals.first() else {
            return Ok(None);
        };
        self.set_key(context, plurals);
        if !self.locate(domain, locale) {
            self.set_key(context, &[first_original]);
            if !self.locate(domain, locale) {
                return Ok(None);
            }
        }
        let form = self
            .found
            .as_ref()
            .expect("found")
            .0
            .parse()
            .plural_form(n)?;
        let v = self.found_bytes();
        let mut forms = v.split(|b| *b == 0);
        let first = v.split(|b| *b == 0).next().unwrap_or(v);
        Ok(Some(
            usize::try_from(form)
                .ok()
                .and_then(|i| forms.nth(i))
                .unwrap_or(first),
        ))
    }

    /// `get_headers()`: every loaded file's headers for the domain, later files winning, names normalized.
    pub fn get_headers(&self, domain: &str) -> Vec<(String, String)> {
        if self.loaded_translations.is_empty() {
            return Vec::new();
        }
        let mut headers: Vec<(String, String)> = Vec::new();
        for moe in self.files(domain, None) {
            for (k, v) in moe.parse().headers() {
                let name = normalize_header(k);
                match headers.iter_mut().find(|(n, _)| *n == name) {
                    Some(slot) => slot.1 = v.clone(),
                    None => headers.push((name, v.clone())),
                }
            }
        }
        headers
    }

    /// `get_entries()`: `array_merge()` of every loaded file's entries for the domain, so integer keys are
    /// renumbered and string keys from later files win.
    pub fn get_entries(&self, domain: &str) -> Vec<(PhpKey, Vec<u8>)> {
        if self.loaded_translations.is_empty() {
            return Vec::new();
        }
        let mut out: Vec<(PhpKey, Vec<u8>)> = Vec::new();
        let mut positions: HashMap<Vec<u8>, usize> = HashMap::new();
        let mut next_int = 0;
        for moe in self.files(domain, None) {
            for (k, v) in moe.parse().entries() {
                match php_key(k) {
                    PhpKey::Int(_) => {
                        out.push((PhpKey::Int(next_int), v.to_vec()));
                        next_int += 1;
                    }
                    PhpKey::Str(s) => match positions.get(&s) {
                        Some(&i) => out[i].1 = v.to_vec(),
                        None => {
                            positions.insert(s.clone(), out.len());
                            out.push((PhpKey::Str(s), v.to_vec()));
                        }
                    },
                }
            }
        }
        out
    }

    /// `has_translation()`.
    pub fn has_translation(&mut self, singular: &[u8], domain: &str, locale: Option<&str>) -> bool {
        self.set_key(b"", &[singular]);
        self.locate(domain, locale)
    }

    /// The file objects loaded for a domain, for the PHP class shells.
    pub fn loaded(&self, domain: &str, locale: Option<&str>) -> Vec<Rc<FileHandle>> {
        self.files(domain, locale)
    }
}

/// `normalize_header()`: `ucfirst()` on each `-` separated part.
fn normalize_header(header: &str) -> String {
    header
        .split('-')
        .map(|p| {
            let mut c = p.chars();
            match c.next() {
                Some(f) => f.to_ascii_uppercase().to_string() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join("-")
}
