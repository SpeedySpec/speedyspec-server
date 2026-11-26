use std::cell::{Cell, RefCell};
use std::rc::Rc;

use speedyspec_cache_memory::MemoryCache;
use speedyspec_data::DbError;
use speedyspec_wp_hooks::{WpHookApi, IgnoreDeprecations};
use speedyspec_wp_hooks_contracts::{Callback, HookName, HookValue};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_object_cache::{CurrentObjectCache, ObjectCachePorts, SiteContext};
use speedyspec_wp_options::{
    Autoload, ExpiredTransients, ExternalObjectCache, InstallingFlag, Multisite, NetworkOptionsApi,
    NetworkPorts, OptionName, OptionRepository, OptionsApi, OptionsDeps, OptionsError,
    ProtectedOptions, Sanitizing, SitemetaRepository, TransientPorts, TransientPrefix,
    TransientsApi,
};
use speedyspec_wp_options_memory::{FixedClock, MemoryOptionRepository, MemorySitemetaRepository};

fn s(v: &str) -> HookValue {
    HookValue::Str(v.into())
}

struct Site {
    multisite: Cell<bool>,
    main: Cell<bool>,
    blog: Cell<i64>,
}
impl Multisite for Site {
    fn is_multisite(&self) -> bool {
        self.multisite.get()
    }
    fn blog_id(&self) -> u64 {
        self.blog.get() as u64
    }
    fn network_id(&self) -> u64 {
        1
    }
    fn is_main_site(&self) -> bool {
        self.main.get()
    }
    fn is_main_network(&self) -> bool {
        true
    }
}
impl SiteContext for Site {
    fn is_multisite(&self) -> bool {
        self.multisite.get()
    }
    fn current_blog_id(&self) -> i64 {
        self.blog.get()
    }
}

struct External(Cell<bool>);
impl ExternalObjectCache for External {
    fn using(&self) -> bool {
        self.0.get()
    }
}

#[derive(Default)]
struct Purges(RefCell<Vec<String>>);
impl ExpiredTransients for Purges {
    fn purge_options(&self, prefix: TransientPrefix, now: i64) -> Result<u64, DbError> {
        self.0
            .borrow_mut()
            .push(format!("options {} {now}", prefix.value()));
        Ok(0)
    }
    fn purge_sitemeta(&self, now: i64) -> Result<u64, DbError> {
        self.0.borrow_mut().push(format!("sitemeta {now}"));
        Ok(0)
    }
}

struct Same;
impl Sanitizing for Same {
    fn sanitize_option(&self, _: &OptionName, value: HookValue) -> Result<HookValue, OptionsError> {
        Ok(value)
    }
}
struct Open;
impl ProtectedOptions for Open {
    fn protect(&self, _: &str) -> Result<(), OptionsError> {
        Ok(())
    }
}

type Log = Rc<RefCell<Vec<(String, Vec<HookValue>)>>>;

struct Rig {
    api: TransientsApi,
    current: Rc<CurrentObjectCache>,
    hooks: Rc<WpHookApi>,
    repo: Rc<MemoryOptionRepository>,
    sitemeta: Rc<MemorySitemetaRepository>,
    site: Rc<Site>,
    external: Rc<External>,
    installing: Rc<InstallingFlag>,
    clock: Rc<FixedClock>,
    purges: Rc<Purges>,
    log: Log,
}

fn rig_with(multisite: bool, persistent: bool) -> Rig {
    let hooks = Rc::new(WpHookApi::new(
        Rc::new(MemoryHookStore::new()),
        Rc::new(IgnoreDeprecations),
    ));
    let site = Rc::new(Site {
        multisite: Cell::new(multisite),
        main: Cell::new(true),
        blog: Cell::new(1),
    });
    let mut cache_ports = ObjectCachePorts::rust();
    cache_ports.site = site.clone();
    let second: Option<Rc<dyn speedyspec_cache::CachingStore>> =
        persistent.then(|| Rc::new(MemoryCache::new()) as Rc<dyn speedyspec_cache::CachingStore>);
    let current = Rc::new(CurrentObjectCache::new(cache_ports, second));
    let repo = Rc::new(MemoryOptionRepository::new());
    // An installed site always has autoloaded options; with none, wp_load_alloptions() loads every row.
    repo.add(&speedyspec_wp_options::Option {
        name: OptionName::new("siteurl").unwrap(),
        value: speedyspec_wp_options::SerializedValue::new(b"http://example.org".to_vec()),
        autoload: Autoload::On,
    })
    .unwrap();
    let installing = Rc::new(InstallingFlag::new());
    let mut deps = OptionsDeps::rust(hooks.clone(), current.clone(), repo.clone());
    deps.multisite = site.clone();
    deps.installing = installing.clone();
    let options = Rc::new(OptionsApi::new(deps));
    let sitemeta = Rc::new(MemorySitemetaRepository::new());
    let network = Rc::new(NetworkOptionsApi::new(
        options.clone(),
        sitemeta.clone(),
        current.clone(),
        hooks.clone(),
        NetworkPorts {
            multisite: site.clone(),
            installing: installing.clone(),
            sanitizing: Rc::new(Same),
            protected: Rc::new(Open),
        },
    ));
    let external = Rc::new(External(Cell::new(persistent)));
    let clock = Rc::new(FixedClock::new(1000));
    let purges = Rc::new(Purges::default());
    let api = TransientsApi::new(
        options,
        network,
        current.clone(),
        hooks.clone(),
        TransientPorts {
            external: external.clone(),
            installing: installing.clone(),
            multisite: site.clone(),
            clock: clock.clone(),
            expired: purges.clone(),
        },
    );
    Rig {
        api,
        current,
        hooks,
        repo,
        sitemeta,
        site,
        external,
        installing,
        clock,
        purges,
        log: Rc::default(),
    }
}

fn rig() -> Rig {
    rig_with(false, false)
}

impl Rig {
    fn row(&self, name: &str) -> Option<(String, Autoload)> {
        let n = OptionName::new(name).unwrap();
        let value = self.repo.value(&n, false).unwrap()?;
        let autoload = self.repo.autoload_of(&n).unwrap().unwrap();
        Some((
            String::from_utf8_lossy(value.as_bytes()).into_owned(),
            autoload,
        ))
    }
    fn record(&self, names: &[&str]) {
        for name in names {
            let (log, n) = (self.log.clone(), name.to_string());
            self.hooks
                .add_filter(
                    &HookName::from(*name),
                    Callback::rust(&format!("record_{name}"), move |args: &[HookValue]| {
                        log.borrow_mut().push((n.clone(), args.to_vec()));
                        Ok(args.first().cloned().unwrap_or(HookValue::Null))
                    }),
                    &HookValue::Int(10),
                    9,
                )
                .unwrap();
        }
    }
    fn names(&self) -> Vec<String> {
        self.log.borrow().iter().map(|(n, _)| n.clone()).collect()
    }
}

const NONE: HookValue = HookValue::Int(0);

#[test]
fn options_storage_basics() {
    let r = rig();
    r.record(&["deleted_transient"]);
    assert!(r.api.set_transient("t", s("v"), &NONE).unwrap());
    assert_eq!(r.row("_transient_t"), Some(("v".into(), Autoload::On)));
    assert_eq!(r.row("_transient_timeout_t"), None);
    assert_eq!(r.api.get_transient("t").unwrap(), s("v"));
    assert!(r.api.delete_transient("t").unwrap());
    assert_eq!(r.row("_transient_t"), None);
    assert_eq!(r.names(), vec!["deleted_transient".to_string()]);
}

#[test]
fn timeout_makes_both_non_autoload() {
    let r = rig();
    assert!(
        r.api
            .set_transient("t", s("v"), &HookValue::Int(60))
            .unwrap()
    );
    assert_eq!(
        r.row("_transient_timeout_t"),
        Some(("1060".into(), Autoload::Off))
    );
    assert_eq!(r.row("_transient_t"), Some(("v".into(), Autoload::Off)));
}

#[test]
fn expired_read_deletes() {
    let r = rig();
    r.api
        .set_transient("t", s("v"), &HookValue::Int(60))
        .unwrap();
    r.clock.set(1061);
    assert_eq!(r.api.get_transient("t").unwrap(), HookValue::Bool(false));
    assert_eq!(r.row("_transient_t"), None);
    assert_eq!(r.row("_transient_timeout_t"), None);
}

#[test]
fn adding_timeout_to_existing_recreates() {
    let r = rig();
    r.api.set_transient("t", s("v"), &NONE).unwrap();
    assert_eq!(r.row("_transient_t").unwrap().1, Autoload::On);
    assert!(
        r.api
            .set_transient("t", s("w"), &HookValue::Int(60))
            .unwrap()
    );
    assert_eq!(r.row("_transient_t"), Some(("w".into(), Autoload::Off)));
    assert_eq!(
        r.row("_transient_timeout_t"),
        Some(("1060".into(), Autoload::Off))
    );
}

#[test]
fn existing_with_timeout_updates_timeout() {
    let r = rig();
    r.api
        .set_transient("t", s("v"), &HookValue::Int(60))
        .unwrap();
    r.clock.set(1010);
    assert!(
        r.api
            .set_transient("t", s("w"), &HookValue::Int(60))
            .unwrap()
    );
    assert_eq!(r.row("_transient_timeout_t").unwrap().0, "1070");
    assert_eq!(r.row("_transient_t").unwrap().0, "w");
}

#[test]
fn object_cache_branch() {
    let r = rig();
    r.external.0.set(true);
    assert!(
        r.api
            .set_transient("t", s("v"), &HookValue::Int(60))
            .unwrap()
    );
    assert_eq!(r.row("_transient_t"), None);
    assert_eq!(
        r.current
            .current()
            .get(&s("t"), &s("transient"), false)
            .unwrap(),
        Some(s("v"))
    );
    assert_eq!(r.api.get_transient("t").unwrap(), s("v"));
    assert!(r.api.delete_transient("t").unwrap());
    assert_eq!(
        r.current
            .current()
            .get(&s("t"), &s("transient"), false)
            .unwrap(),
        None
    );
}

#[test]
fn installing_uses_object_cache() {
    let r = rig();
    r.installing.set(true);
    assert!(r.api.set_transient("t", s("v"), &NONE).unwrap());
    assert_eq!(
        r.current
            .current()
            .get(&s("t"), &s("transient"), false)
            .unwrap(),
        Some(s("v"))
    );
}

#[test]
fn set_hooks_in_order() {
    let r = rig();
    r.record(&[
        "pre_set_transient_t",
        "expiration_of_transient_t",
        "set_transient_t",
        "set_transient",
        "setted_transient",
    ]);
    r.api
        .set_transient("t", s("v"), &HookValue::Int(60))
        .unwrap();
    let (t, v, e) = (s("t"), s("v"), HookValue::Int(60));
    assert_eq!(
        *r.log.borrow(),
        vec![
            (
                "pre_set_transient_t".to_string(),
                vec![v.clone(), e.clone(), t.clone()]
            ),
            (
                "expiration_of_transient_t".to_string(),
                vec![e.clone(), v.clone(), t.clone()]
            ),
            (
                "set_transient_t".to_string(),
                vec![v.clone(), e.clone(), t.clone()]
            ),
            (
                "set_transient".to_string(),
                vec![t.clone(), v.clone(), e.clone()]
            ),
            (
                "setted_transient".to_string(),
                vec![t.clone(), v.clone(), e.clone()]
            ),
        ]
    );
}

#[test]
fn pre_transient_short_circuits() {
    let r = rig();
    r.hooks
        .add_filter(
            &HookName::from("pre_transient_t"),
            Callback::rust("short", |_: &[HookValue]| {
                Ok(HookValue::Str("short".into()))
            }),
            &HookValue::Int(10),
            1,
        )
        .unwrap();
    let before = r.repo.statements();
    assert_eq!(r.api.get_transient("t").unwrap(), s("short"));
    assert_eq!(r.repo.statements(), before);
}

#[test]
fn odd_expirations() {
    for (expiration, timeout) in [
        (s("60"), Some("1060")),
        (s("abc"), None),
        (HookValue::Float(60.9), Some("1060")),
        (HookValue::Int(-10), Some("990")),
    ] {
        let r = rig();
        r.api.set_transient("t", s("v"), &expiration).unwrap();
        assert_eq!(
            r.row("_transient_timeout_t").map(|(v, _)| v),
            timeout.map(str::to_string),
            "{expiration:?}"
        );
    }
    let r = rig();
    r.api
        .set_transient("t", s("v"), &HookValue::Int(-10))
        .unwrap();
    assert_eq!(r.api.get_transient("t").unwrap(), HookValue::Bool(false));
}

#[test]
fn site_transient_single_site_in_options() {
    let r = rig();
    assert!(r.api.set_site_transient("t", s("v"), &NONE).unwrap());
    assert_eq!(
        r.row("_site_transient_t"),
        Some(("v".into(), Autoload::Off))
    );
    assert_eq!(r.api.get_site_transient("t").unwrap(), s("v"));
    assert!(r.api.delete_site_transient("t").unwrap());
    assert_eq!(r.row("_site_transient_t"), None);
}

#[test]
fn site_transient_multisite_in_sitemeta() {
    let r = rig_with(true, false);
    assert!(
        r.api
            .set_site_transient("t", s("v"), &HookValue::Int(60))
            .unwrap()
    );
    assert_eq!(
        r.sitemeta
            .find(1, "_site_transient_t")
            .unwrap()
            .map(|m| m.value.as_bytes().to_vec()),
        Some(b"v".to_vec())
    );
    assert_eq!(
        r.sitemeta
            .find(1, "_site_transient_timeout_t")
            .unwrap()
            .map(|m| m.value.as_bytes().to_vec()),
        Some(b"1060".to_vec())
    );
    assert_eq!(r.row("_site_transient_t"), None);
    assert_eq!(r.api.get_site_transient("t").unwrap(), s("v"));
}

#[test]
fn site_core_names_skip_timeout() {
    let r = rig();
    r.record(&[
        "pre_option__site_transient_timeout_update_core",
        "pre_site_option__site_transient_timeout_update_core",
    ]);
    r.api
        .set_site_transient("update_core", s("v"), &HookValue::Int(60))
        .unwrap();
    r.clock.set(2000);
    r.log.borrow_mut().clear();
    // Past its timeout, still read: the core update checks never expire by timeout.
    assert_eq!(r.api.get_site_transient("update_core").unwrap(), s("v"));
    assert!(r.names().is_empty(), "{:?}", r.names());
}

#[test]
fn site_pre_set_args() {
    let r = rig();
    r.record(&["pre_set_site_transient_t"]);
    r.api
        .set_site_transient("t", s("v"), &HookValue::Int(60))
        .unwrap();
    assert_eq!(
        *r.log.borrow(),
        vec![("pre_set_site_transient_t".to_string(), vec![s("v"), s("t")])]
    );
}

#[test]
fn site_transient_is_global_across_blogs() {
    let r = rig_with(true, true);
    assert!(
        r.api
            .set_site_transient("shared", s("site"), &NONE)
            .unwrap()
    );
    assert!(r.api.set_transient("shared", s("blog"), &NONE).unwrap());
    r.site.blog.set(2);
    r.current.current().switch_to_blog(2);
    assert_eq!(r.api.get_site_transient("shared").unwrap(), s("site"));
    assert_eq!(
        r.api.get_transient("shared").unwrap(),
        HookValue::Bool(false)
    );
}

#[test]
fn delete_expired_branches() {
    let r = rig();
    r.external.0.set(true);
    r.api.delete_expired_transients(false).unwrap();
    assert!(r.purges.0.borrow().is_empty());
    r.external.0.set(false);
    r.api.delete_expired_transients(false).unwrap();
    assert_eq!(
        *r.purges.0.borrow(),
        vec!["options _transient_ 1000", "options _site_transient_ 1000"]
    );

    let r = rig_with(true, false);
    r.api.delete_expired_transients(false).unwrap();
    assert_eq!(
        *r.purges.0.borrow(),
        vec!["options _transient_ 1000", "sitemeta 1000"]
    );

    let r = rig_with(true, false);
    r.site.main.set(false);
    r.api.delete_expired_transients(true).unwrap();
    assert_eq!(*r.purges.0.borrow(), vec!["options _transient_ 1000"]);
}

#[test]
fn php_int_casts() {
    use speedyspec_wp_options::php_int;
    for (value, expected) in [
        (HookValue::Null, 0),
        (HookValue::Bool(true), 1),
        (HookValue::Float(60.9), 60),
        (HookValue::Float(-1.5), -1),
        (s("60"), 60),
        (s(" 12abc"), 12),
        (s("1e3"), 1000),
        (s("abc"), 0),
        (s(""), 0),
    ] {
        assert_eq!(php_int(&value), expected, "{value:?}");
    }
}
