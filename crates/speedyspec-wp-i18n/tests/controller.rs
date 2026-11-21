//! The translation controller, with the real readers, file probe and cache (`WP_Translation_Controller`'s behaviour).

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use speedyspec_wp_i18n::{FileRef, PhpKey, TranslationController, TranslationFile};
use speedyspec_wp_i18n_contracts::{RawTranslations, ReadError, TranslationFileReader};
use speedyspec_wp_i18n_files::{ByExtension, StdFileProbe};
use speedyspec_wp_i18n_memory::MemoryTranslationFileCache;

fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../speedyspec-wp-i18n-files/tests/data")
        .join(name)
}

/// Counts reads, so tests can see the cache at work.
struct Counting {
    inner: ByExtension,
    reads: Cell<usize>,
}

impl TranslationFileReader for Counting {
    fn read(&self, path: &Path) -> Result<RawTranslations, ReadError> {
        self.reads.set(self.reads.get() + 1);
        self.inner.read(path)
    }
}

fn tr(
    c: &mut TranslationController,
    text: &[u8],
    context: &[u8],
    domain: &str,
    locale: Option<&str>,
) -> Option<Vec<u8>> {
    c.translate(text, context, domain, locale)
        .map(<[u8]>::to_vec)
}

fn trp(
    c: &mut TranslationController,
    singular: &[u8],
    plural: &[u8],
    n: i64,
    context: &[u8],
    domain: &str,
    locale: Option<&str>,
) -> Result<Option<Vec<u8>>, speedyspec_wp_i18n::PluralError> {
    c.translate_plural(singular, plural, n, context, domain, locale)
        .map(|o| o.map(<[u8]>::to_vec))
}

fn controller() -> (
    TranslationController,
    Rc<Counting>,
    Rc<MemoryTranslationFileCache<TranslationFile>>,
) {
    let reader = Rc::new(Counting {
        inner: ByExtension::default(),
        reads: Cell::new(0),
    });
    let cache = Rc::new(MemoryTranslationFileCache::default());
    (
        TranslationController::new(reader.clone(), Rc::new(StdFileProbe), cache.clone()),
        reader,
        cache,
    )
}

#[test]
fn load_and_translate() {
    let (mut c, _, _) = controller();
    assert!(c.load_file(&data("simple.mo"), "wp-tests-domain", None));
    assert!(c.is_textdomain_loaded("wp-tests-domain", None));
    assert_eq!(
        tr(&mut c, b"baba", b"", "wp-tests-domain", None),
        Some(b"dyado".to_vec())
    );
    assert_eq!(tr(&mut c, b"baba", b"", "other", None), None);
    assert_eq!(c.locale(), "en_US");
}

#[test]
fn missing_file_does_not_load() {
    let (mut c, _, _) = controller();
    assert!(!c.load_file(&data("nope.mo"), "d", None));
    assert!(!c.load_file(&data("example-simple.po"), "d", None));
    assert!(!c.is_textdomain_loaded("d", None));
}

#[test]
fn broken_file_loads_then_unloads_on_first_lookup() {
    // WordPress parses lazily: load_file() succeeds, and the error shows on the first lookup, which unloads it.
    let (mut c, _, _) = controller();
    assert!(c.load_file(&data("broken-magic.mo"), "d", None));
    assert!(c.is_textdomain_loaded("d", None));
    assert_eq!(tr(&mut c, b"baba", b"", "d", None), None);
    assert!(!c.is_textdomain_loaded("d", None));
}

#[test]
fn same_file_two_domains_shares_one_file() {
    let (mut c, reader, cache) = controller();
    assert!(c.load_file(&data("simple.mo"), "a", None));
    assert!(c.load_file(&data("simple.mo"), "b", None));
    assert_eq!(cache.len(), 1);
    assert_eq!(reader.reads.get(), 1);
}

#[test]
fn files_are_reused_across_requests() {
    let (mut c, reader, _) = controller();
    assert!(c.load_file(&data("simple.mo"), "a", None));
    c.reset();
    assert!(!c.is_textdomain_loaded("a", None));
    assert!(c.load_file(&data("simple.mo"), "a", None));
    assert_eq!(tr(&mut c, b"baba", b"", "a", None), Some(b"dyado".to_vec()));
    assert_eq!(reader.reads.get(), 1);
}

#[test]
fn already_loaded_returns_prior_result() {
    let (mut c, _, _) = controller();
    assert!(c.load_file(&data("simple.mo"), "a", None));
    assert!(c.load_file(&data("simple.mo"), "a", None));
    assert_eq!(c.get_entries("a").len(), 2, "loaded once, not twice");
}

#[test]
fn first_loaded_file_wins() {
    let (mut c, _, _) = controller();
    assert!(c.load_file(&data("pomo-overload.mo"), "d", None));
    assert!(c.load_file(&data("pomo-de_DE-2.8.mo"), "d", None));
    let first = tr(&mut c, b"Dashboard", b"", "d", None);
    let (mut c2, _, _) = controller();
    assert!(c2.load_file(&data("pomo-overload.mo"), "d", None));
    assert_eq!(first, tr(&mut c2, b"Dashboard", b"", "d", None));
}

#[test]
fn plural_falls_back_to_singular_key_and_form_zero() {
    let (mut c, _, _) = controller();
    assert!(c.load_file(&data("plural.mo"), "d", None));
    assert_eq!(
        trp(&mut c, b"one dragon", b"%d dragons", 1, b"", "d", None),
        Ok(Some(b"oney dragoney".to_vec()))
    );
    assert_eq!(
        trp(&mut c, b"one dragon", b"%d dragons", 3, b"", "d", None),
        Ok(Some(b"twoey dragoney".to_vec()))
    );
    assert_eq!(
        trp(&mut c, b"missing", b"missings", 3, b"", "d", None),
        Ok(None)
    );
}

#[test]
fn context_lookup() {
    let (mut c, _, _) = controller();
    assert!(c.load_file(&data("pomo-context.mo"), "d", None));
    let entries = c.get_entries("d");
    let (key, value) = entries
        .iter()
        .find(|(k, _)| matches!(k, PhpKey::Str(s) if s.contains(&4)))
        .expect("a context key");
    let PhpKey::Str(key) = key else {
        unreachable!()
    };
    let split = key.iter().position(|b| *b == 4).unwrap();
    let first = value.split(|b| *b == 0).next().unwrap().to_vec();
    assert_eq!(
        tr(&mut c, &key[split + 1..], &key[..split], "d", None),
        Some(first)
    );
}

#[test]
fn locales_are_separate() {
    let (mut c, _, _) = controller();
    assert!(c.load_file(&data("simple.mo"), "d", Some("de_DE")));
    assert!(!c.is_textdomain_loaded("d", None));
    assert!(c.is_textdomain_loaded("d", Some("de_DE")));
    assert!(c.has_translation(b"baba", "d", Some("de_DE")));
    assert!(!c.has_translation(b"baba", "d", None));
    c.set_locale("de_DE");
    assert!(c.has_translation(b"baba", "d", None));
}

#[test]
fn unload_non_existent_files_and_textdomains() {
    let (mut c, _, _) = controller();
    assert!(!c.unload_textdomain("foobarbaz", None));
    assert!(!c.unload_textdomain("foobarbaz", Some("es_ES")));
    assert!(!c.unload_file(FileRef::Path(&data("fa_IR.mo")), "default", None));
    // With a locale, unload_file() answers true even when nothing matched, as WordPress does.
    assert!(c.unload_file(FileRef::Path(&data("fa_IR.mo")), "default", Some("es_ES")));
}

#[test]
fn unload_file_and_textdomain() {
    let (mut c, _, _) = controller();
    assert!(c.load_file(&data("simple.mo"), "d", None));
    assert!(c.load_file(&data("plural.mo"), "d", None));
    assert!(c.unload_file(FileRef::Path(&data("simple.mo")), "d", None));
    assert_eq!(tr(&mut c, b"baba", b"", "d", None), None);
    assert!(c.is_textdomain_loaded("d", None));
    assert!(c.unload_textdomain("d", None));
    assert!(!c.is_textdomain_loaded("d", None));
    // Unloaded, it loads again.
    assert!(c.load_file(&data("simple.mo"), "d", None));
    assert_eq!(tr(&mut c, b"baba", b"", "d", None), Some(b"dyado".to_vec()));
}

#[test]
fn headers_are_merged_and_normalized() {
    let (mut c, _, _) = controller();
    assert!(c.get_headers("d").is_empty());
    assert!(c.load_file(&data("fa_IR.mo"), "d", None));
    let headers = c.get_headers("d");
    assert!(
        headers.iter().any(|(k, _)| k == "Plural-Forms"),
        "{headers:?}"
    );
    assert!(headers.iter().all(|(k, _)| {
        k.split('-')
            .all(|p| p.chars().next().is_none_or(|c| !c.is_ascii_lowercase()))
    }));
}

#[test]
fn get_entries_renumbers_integer_keys_as_array_merge_does() {
    let (mut c, _, _) = controller();
    assert!(c.load_file(&data("escapes.l10n.php"), "d", None));
    assert!(c.load_file(&data("escapes.l10n.php"), "e", None));
    let entries = c.get_entries("d");
    assert!(
        entries.contains(&(PhpKey::Int(0), b"seven".to_vec())),
        "{entries:?}"
    );
    assert!(entries.contains(&(PhpKey::Str(b"it's".to_vec()), b"a\\\\b".to_vec())));
}
