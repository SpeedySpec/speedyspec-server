//! Script translations: where `_load_script_textdomain_from_src()` looks, the filters it fires, and the per-worker
//! JSON cache.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use speedyspec_wp_hooks::{IgnoreDeprecations, WpHookApi};
use speedyspec_wp_hooks_contracts::{Callback, HookName, HookValue};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_i18n::{
    I18nApi, I18nError, Relative, ScriptFiles, TranslationController, json_filename, relative_path,
};
use speedyspec_wp_i18n_contracts::{FileIdentity, FileProbe, PortError, ScriptSources, UrlParts};
use speedyspec_wp_i18n_files::{ByExtension, StdFileProbe};
use speedyspec_wp_i18n_memory::MemoryTranslationFileCache;

fn s(v: &str) -> HookValue {
    HookValue::Str(v.into())
}

fn url(scheme: Option<&str>, host: Option<&str>, path: Option<&str>) -> UrlParts {
    UrlParts {
        scheme: scheme.map(Into::into),
        host: host.map(Into::into),
        path: path.map(Into::into),
    }
}

fn languages() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/php/data/languages")
}

const THEME_ROOT: &str = "/srv/wp-content/themes";

fn content() -> UrlParts {
    url(Some("http"), Some("example.org"), Some("/wp-content"))
}

fn plugins() -> UrlParts {
    url(
        Some("http"),
        Some("example.org"),
        Some("/wp-content/plugins"),
    )
}

fn site() -> UrlParts {
    url(Some("http"), Some("example.org"), None)
}

fn rel(
    src: UrlParts,
    content: &UrlParts,
    plugins: &UrlParts,
    site: &UrlParts,
) -> (Option<String>, Option<&'static str>) {
    match relative_path(&src, Some(content), Some(plugins), Some(site), THEME_ROOT) {
        Relative::Path {
            relative,
            languages_suffix,
        } => (
            relative.map(|r| String::from_utf8(r).unwrap()),
            languages_suffix,
        ),
        Relative::Unparseable => panic!("unexpected"),
    }
}

#[test]
fn relative_path_matches_wordpress() {
    let (c, p, si) = (content(), plugins(), site());
    assert_eq!(
        rel(
            url(None, None, Some("/wp-includes/js/script.js")),
            &c,
            &p,
            &si
        ),
        (Some("wp-includes/js/script.js".into()), None)
    );
    assert_eq!(
        rel(
            url(
                None,
                None,
                Some("/wp-content/plugins/my-plugin/js/script.js")
            ),
            &c,
            &p,
            &si
        ),
        (Some("js/script.js".into()), Some("plugins"))
    );
    assert_eq!(
        rel(
            url(None, None, Some("/wp-content/themes/my-theme/js/script.js")),
            &c,
            &p,
            &si
        ),
        (Some("js/script.js".into()), Some("themes"))
    );
    assert_eq!(
        rel(
            url(Some("https"), Some("cdn.example.com"), Some("/x.js")),
            &c,
            &p,
            &si
        ),
        (None, None)
    );
    // content_url() with no path: the whole src path is relative to it.
    let no_path = url(Some("https"), Some("example.org"), None);
    assert_eq!(
        rel(
            url(
                Some("https"),
                Some("example.org"),
                Some("/plugins/my-plugin/js/script.js")
            ),
            &no_path,
            &p,
            &si
        ),
        (Some("js/script.js".into()), Some("plugins"))
    );
    // plugins_url() on another host.
    let other = url(Some("https"), Some("plugins.example.com"), None);
    assert_eq!(
        rel(
            url(
                Some("https"),
                Some("plugins.example.com"),
                Some("/my-plugin/js/script.js")
            ),
            &c,
            &other,
            &si
        ),
        (Some("js/script.js".into()), Some("plugins"))
    );
    // WordPress in a subdirectory.
    let sub = url(Some("http"), Some("example.org"), Some("/wp"));
    assert_eq!(
        rel(
            url(None, None, Some("/wp/wp-includes/js/script.js")),
            &c,
            &p,
            &sub
        ),
        (Some("wp-includes/js/script.js".into()), None)
    );
    assert_eq!(
        rel(url(None, None, Some("/other/x.js")), &c, &p, &sub),
        (None, None)
    );
}

#[test]
fn a_site_path_with_extra_slashes_slices_as_php_does() {
    // PHP's substr() past the end is '', and inside a multibyte character it keeps the raw bytes.
    let (c, p) = (content(), plugins());
    let site = url(Some("http"), Some("example.org"), Some("/wp//"));
    assert_eq!(
        rel(url(None, None, Some("/wp/")), &c, &p, &site),
        (Some(String::new()), None)
    );
    match relative_path(
        &url(None, None, Some("/wp/é.js")),
        Some(&c),
        Some(&p),
        Some(&site),
        THEME_ROOT,
    ) {
        Relative::Path { relative, .. } => assert_eq!(relative, Some(b"\xa9.js".to_vec())),
        other => panic!("{other:?}"),
    }
}

#[test]
fn json_filename_strips_min() {
    assert_eq!(
        json_filename("en_US", b"wp-includes/js/script.min.js"),
        "en_US-813e104eb47e13dd4cc5af844c618754.json"
    );
    assert_eq!(
        json_filename("en_US", b"wp-includes/js/script.js"),
        "en_US-813e104eb47e13dd4cc5af844c618754.json"
    );
}

/// Answers as WordPress's test install would, with `src` parsed by a tiny parser good enough for these tests.
struct Sources {
    locale: String,
    registry: Option<String>,
}

impl ScriptSources for Sources {
    fn determine_locale(&self) -> Result<String, PortError> {
        Ok(self.locale.clone())
    }
    fn registry_path(&self, _domain: &str, _locale: &str) -> Result<Option<String>, PortError> {
        Ok(self.registry.clone())
    }
    fn parse_url(&self, u: &str) -> Result<Option<UrlParts>, PortError> {
        if u == "http:///broken" {
            return Ok(None);
        }
        Ok(Some(match u.split_once("://") {
            Some((scheme, rest)) => {
                let (host, path) = match rest.find('/') {
                    Some(i) => (&rest[..i], Some(&rest[i..])),
                    None => (rest, None),
                };
                url(Some(scheme), Some(host), path)
            }
            None => url(None, None, Some(u)),
        }))
    }
    fn content_url(&self) -> Result<String, PortError> {
        Ok("http://example.org/wp-content".into())
    }
    fn plugins_url(&self) -> Result<String, PortError> {
        Ok("http://example.org/wp-content/plugins".into())
    }
    fn site_url(&self) -> Result<String, PortError> {
        Ok("http://example.org".into())
    }
    fn theme_root(&self) -> Result<String, PortError> {
        Ok(THEME_ROOT.into())
    }
    fn lang_dir(&self) -> String {
        languages().display().to_string()
    }
}

/// Counts identity lookups and reads on top of the real probe.
#[derive(Default)]
struct Counting {
    identities: Cell<usize>,
}

impl FileProbe for Counting {
    fn realpath(&self, path: &Path) -> Option<PathBuf> {
        StdFileProbe.realpath(path)
    }
    fn is_readable(&self, path: &Path) -> bool {
        StdFileProbe.is_readable(path)
    }
    fn identity(&self, path: &Path) -> Option<FileIdentity> {
        self.identities.set(self.identities.get() + 1);
        StdFileProbe.identity(path)
    }
    fn glob(&self, dir: &Path, pattern: &str) -> Vec<PathBuf> {
        StdFileProbe.glob(dir, pattern)
    }
}

type Log = Rc<RefCell<Vec<(String, Vec<HookValue>)>>>;

fn api() -> (I18nApi, Rc<WpHookApi>, Log) {
    let hooks = Rc::new(WpHookApi::new(
        Rc::new(MemoryHookStore::new()),
        Rc::new(IgnoreDeprecations),
    ));
    let controller = TranslationController::new(
        Rc::new(ByExtension::default()),
        Rc::new(StdFileProbe),
        Rc::new(MemoryTranslationFileCache::default()),
    );
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    for name in [
        "pre_load_script_translations",
        "load_script_translation_file",
        "load_script_translations",
        "load_script_textdomain_relative_path",
    ] {
        let log = log.clone();
        hooks
            .add_filter(
                &HookName::new(s(name)),
                Callback::rust(name, move |args: &[HookValue]| {
                    log.borrow_mut().push((name.to_string(), args.to_vec()));
                    Ok(args[0].clone())
                }),
                &HookValue::Int(10),
                4,
            )
            .unwrap();
    }
    (
        I18nApi::new(hooks.clone(), Rc::new(RefCell::new(controller))),
        hooks,
        log,
    )
}

fn files() -> ScriptFiles {
    ScriptFiles::new(
        Rc::new(StdFileProbe),
        Rc::new(MemoryTranslationFileCache::default()),
    )
}

#[test]
fn filters_fire_in_order_and_the_md5_file_loads() {
    let (api, _, log) = api();
    let sources = Sources {
        locale: "en_US".into(),
        registry: None,
    };
    let got = api
        .load_script_textdomain_from_src(
            &sources,
            &files(),
            "h",
            "/wp-includes/js/script.js",
            "default",
            "",
            false,
        )
        .unwrap();
    let want =
        std::fs::read_to_string(languages().join("en_US-813e104eb47e13dd4cc5af844c618754.json"))
            .unwrap();
    assert_eq!(got, s(&want));
    let names: Vec<String> = log.borrow().iter().map(|(n, _)| n.clone()).collect();
    assert_eq!(
        names,
        vec![
            "load_script_textdomain_relative_path",
            "pre_load_script_translations",
            "load_script_translation_file",
            "load_script_translations",
        ]
    );
    assert_eq!(
        log.borrow()[0].1,
        vec![
            s("wp-includes/js/script.js"),
            s("/wp-includes/js/script.js"),
            HookValue::Bool(false)
        ]
    );
}

#[test]
fn a_given_path_is_tried_first_by_handle() {
    let (api, _, log) = api();
    let sources = Sources {
        locale: "en_US".into(),
        registry: None,
    };
    let path = languages().display().to_string();
    let got = api
        .load_script_textdomain_from_src(
            &sources,
            &files(),
            "script-handle",
            "/x.js",
            "admin",
            &format!("{path}/"),
            true,
        )
        .unwrap();
    let want = std::fs::read_to_string(languages().join("admin-en_US-script-handle.json")).unwrap();
    assert_eq!(got, s(&want));
    assert_eq!(
        log.borrow()[0].1[1],
        s(&format!("{path}/admin-en_US-script-handle.json"))
    );
}

#[test]
fn unparseable_src_returns_false_through_the_filters() {
    let (api, _, log) = api();
    let sources = Sources {
        locale: "en_US".into(),
        registry: None,
    };
    let got = api
        .load_script_textdomain_from_src(
            &sources,
            &files(),
            "h",
            "http:///broken",
            "default",
            "",
            false,
        )
        .unwrap();
    assert_eq!(got, HookValue::Bool(false));
    assert_eq!(
        log.borrow()[0],
        (
            "pre_load_script_translations".into(),
            vec![
                HookValue::Null,
                HookValue::Bool(false),
                s("h"),
                s("default")
            ]
        )
    );
}

#[test]
fn pre_filter_short_circuits_with_any_value() {
    let (api, hooks, _) = api();
    let array = HookValue::Array(vec![(
        speedyspec_hooks::Key::Str("a".into()),
        HookValue::Int(1),
    )]);
    let answer = array.clone();
    hooks
        .add_filter(
            &HookName::new(s("pre_load_script_translations")),
            Callback::rust("short", move |_: &[HookValue]| Ok(answer.clone())),
            &HookValue::Int(20),
            1,
        )
        .unwrap();
    let got = api
        .load_script_translations(&files(), s("/nope.json"), "h", "default")
        .unwrap();
    assert_eq!(got, array);
}

#[test]
fn unreadable_or_empty_file_returns_false() {
    let (api, _, _) = api();
    assert_eq!(
        api.load_script_translations(&files(), s("/nope.json"), "h", "d")
            .unwrap(),
        HookValue::Bool(false)
    );
    assert_eq!(
        api.load_script_translations(&files(), s(""), "h", "d")
            .unwrap(),
        HookValue::Bool(false)
    );
    assert_eq!(
        api.load_script_translations(&files(), HookValue::Bool(false), "h", "d")
            .unwrap(),
        HookValue::Bool(false)
    );
}

#[test]
fn an_array_file_is_a_type_error() {
    let (api, _, _) = api();
    let array = HookValue::Array(vec![(speedyspec_hooks::Key::Int(0), HookValue::Int(1))]);
    match api.load_script_translations(&files(), array, "h", "d") {
        Err(I18nError::Type(m)) => assert_eq!(
            m,
            "is_readable(): Argument #1 ($filename) must be of type string, array given"
        ),
        other => panic!("{other:?}"),
    }
}

#[test]
fn script_cache_hits_unchanged_and_rereads_after_rewrite() {
    let dir = std::env::temp_dir().join(format!("speedyspec-script-cache-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("a.json");
    std::fs::write(&file, b"{\"v\":1}").unwrap();
    let probe = Rc::new(Counting::default());
    let files = ScriptFiles::new(
        probe.clone(),
        Rc::new(MemoryTranslationFileCache::default()),
    );
    let first = files.read(&file).unwrap();
    let again = files.read(&file).unwrap();
    assert!(
        Rc::ptr_eq(&first, &again),
        "an unchanged file is served from the cache"
    );
    std::fs::write(&file, b"{\"v\":22}").unwrap();
    assert_eq!(&**files.read(&file).unwrap(), b"{\"v\":22}");
    assert!(files.read(&dir.join("missing.json")).is_none());
    std::fs::remove_dir_all(&dir).unwrap();
}
