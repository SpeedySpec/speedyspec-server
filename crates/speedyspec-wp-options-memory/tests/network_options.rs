use std::cell::{Cell, RefCell};
use std::rc::Rc;

use speedyspec_data::DbError;
use speedyspec_wp_hooks::{WpHookApi, IgnoreDeprecations};
use speedyspec_wp_hooks_contracts::{Callback, HookName, HookValue, Key};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_object_cache::{CurrentObjectCache, ObjectCachePorts};
use speedyspec_wp_options::Option as OptionEntity;
use speedyspec_wp_options::{
    Autoload, InstallingFlag, Multisite, NetworkMeta, NetworkOptionsApi, NetworkPorts, OptionName,
    OptionRepository, OptionsApi, OptionsDeps, OptionsError, ProtectedOptions, Sanitizing,
    SerializedValue, SitemetaRepository,
};
use speedyspec_wp_options_memory::{MemoryOptionRepository, MemorySitemetaRepository};

fn s(v: &str) -> HookValue {
    HookValue::Str(v.into())
}

struct Site {
    multisite: Cell<bool>,
    network: Cell<u64>,
}
impl Multisite for Site {
    fn is_multisite(&self) -> bool {
        self.multisite.get()
    }
    fn blog_id(&self) -> u64 {
        1
    }
    fn network_id(&self) -> u64 {
        self.network.get()
    }
    fn is_main_site(&self) -> bool {
        true
    }
    fn is_main_network(&self) -> bool {
        true
    }
}

/// The memory repository, counting calls and recording each `find_many`.
#[derive(Default)]
struct Counting {
    inner: MemorySitemetaRepository,
    calls: Cell<u32>,
    find_many: RefCell<Vec<Vec<String>>>,
}
impl Counting {
    fn seed(&self, network_id: i64, key: &str, value: &str) {
        self.inner
            .add(NetworkMeta {
                network_id,
                key: key.into(),
                value: SerializedValue::new(value.as_bytes().to_vec()),
            })
            .unwrap();
    }
    fn hit(&self) {
        self.calls.set(self.calls.get() + 1);
    }
}
impl SitemetaRepository for Counting {
    fn find(&self, network_id: i64, key: &str) -> Result<Option<NetworkMeta>, DbError> {
        self.hit();
        self.inner.find(network_id, key)
    }
    fn find_many(&self, network_id: i64, keys: &[String]) -> Result<Vec<NetworkMeta>, DbError> {
        self.hit();
        self.find_many.borrow_mut().push(keys.to_vec());
        self.inner.find_many(network_id, keys)
    }
    fn exists(&self, network_id: i64, key: &str) -> Result<bool, DbError> {
        self.hit();
        self.inner.exists(network_id, key)
    }
    fn add(&self, meta: NetworkMeta) -> Result<bool, DbError> {
        self.hit();
        self.inner.add(meta)
    }
    fn update(&self, meta: NetworkMeta) -> Result<u64, DbError> {
        self.hit();
        self.inner.update(meta)
    }
    fn remove(&self, network_id: i64, key: &str) -> Result<u64, DbError> {
        self.hit();
        self.inner.remove(network_id, key)
    }
}

struct Same;
impl Sanitizing for Same {
    fn sanitize_option(&self, _: &OptionName, value: HookValue) -> Result<HookValue, OptionsError> {
        Ok(value)
    }
}

#[derive(Default)]
struct Protect(RefCell<Vec<String>>);
impl ProtectedOptions for Protect {
    fn protect(&self, name: &str) -> Result<(), OptionsError> {
        self.0.borrow_mut().push(name.into());
        if name == "alloptions" || name == "notoptions" {
            return Err(OptionsError::Protected(format!("{name} is protected")));
        }
        Ok(())
    }
}

type Log = Rc<RefCell<Vec<(String, Vec<HookValue>)>>>;

struct Rig {
    api: NetworkOptionsApi,
    options: Rc<OptionsApi>,
    current: Rc<CurrentObjectCache>,
    hooks: Rc<WpHookApi>,
    repo: Rc<MemoryOptionRepository>,
    sitemeta: Rc<Counting>,
    protect: Rc<Protect>,
    log: Log,
}

fn rig(multisite: bool) -> Rig {
    let hooks = Rc::new(WpHookApi::new(
        Rc::new(MemoryHookStore::new()),
        Rc::new(IgnoreDeprecations),
    ));
    let current = Rc::new(CurrentObjectCache::new(ObjectCachePorts::rust(), None));
    let repo = Rc::new(MemoryOptionRepository::new());
    let site = Rc::new(Site {
        multisite: Cell::new(multisite),
        network: Cell::new(1),
    });
    let mut deps = OptionsDeps::rust(hooks.clone(), current.clone(), repo.clone());
    deps.multisite = site.clone();
    let options = Rc::new(OptionsApi::new(deps));
    let sitemeta = Rc::new(Counting::default());
    let protect = Rc::new(Protect::default());
    let ports = NetworkPorts {
        multisite: site,
        installing: Rc::new(InstallingFlag::new()),
        sanitizing: Rc::new(Same),
        protected: protect.clone(),
    };
    let api = NetworkOptionsApi::new(
        options.clone(),
        sitemeta.clone(),
        current.clone(),
        hooks.clone(),
        ports,
    );
    Rig {
        api,
        options,
        current,
        hooks,
        repo,
        sitemeta,
        protect,
        log: Rc::default(),
    }
}

impl Rig {
    fn cached(&self, key: &str) -> Option<HookValue> {
        self.current
            .current()
            .get(&s(key), &s("site-options"), false)
            .unwrap()
    }
    fn notoption(&self, network: i64, name: &str) -> bool {
        match self.cached(&format!("{network}:notoptions")) {
            Some(HookValue::Array(items)) => items.iter().any(|(k, _)| *k == Key::Str(name.into())),
            _ => false,
        }
    }
    /// Logs each listed hook's name and arguments, passing the value through.
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
    fn logged(&self) -> Vec<(String, Vec<HookValue>)> {
        self.log.borrow_mut().drain(..).collect()
    }
}

#[test]
fn single_site_falls_back_to_options() {
    let r = rig(false);
    assert!(
        r.api
            .add_network_option(&HookValue::Null, "x", s("v"))
            .unwrap()
    );
    let name = OptionName::new("x").unwrap();
    assert_eq!(r.repo.autoload_of(&name).unwrap(), Some(Autoload::Off));
    assert_eq!(
        r.api
            .get_network_option(&HookValue::Null, "x", HookValue::Bool(false))
            .unwrap(),
        s("v")
    );
    assert!(
        r.api
            .update_network_option(&HookValue::Null, "x", s("w"))
            .unwrap()
    );
    assert_eq!(
        r.repo
            .value(&name, false)
            .unwrap()
            .map(|v| v.as_bytes().to_vec()),
        Some(b"w".to_vec())
    );
    assert!(r.api.delete_network_option(&HookValue::Null, "x").unwrap());
    assert_eq!(r.repo.value(&name, false).unwrap(), None);
    assert_eq!(r.sitemeta.calls.get(), 0);
}

#[test]
fn multisite_uses_sitemeta() {
    let r = rig(true);
    r.sitemeta.seed(2, "x", "v");
    assert_eq!(
        r.api
            .get_network_option(&HookValue::Int(2), "x", HookValue::Bool(false))
            .unwrap(),
        s("v")
    );
    assert_eq!(r.cached("2:x"), Some(s("v")));
    assert_eq!(r.repo.statements(), 0);
}

#[test]
fn notoptions_per_network() {
    let r = rig(true);
    assert_eq!(
        r.api
            .get_network_option(&HookValue::Int(1), "x", HookValue::Bool(false))
            .unwrap(),
        HookValue::Bool(false)
    );
    assert!(r.notoption(1, "x"));
    assert_eq!(r.cached("2:notoptions"), None);
}

#[test]
fn found_option_sets_empty_notoptions() {
    let r = rig(true);
    r.sitemeta.seed(1, "x", "v");
    r.api
        .get_network_option(&HookValue::Int(1), "x", HookValue::Bool(false))
        .unwrap();
    assert_eq!(r.cached("1:notoptions"), Some(HookValue::Array(Vec::new())));
}

#[test]
fn add_and_update_clear_notoptions() {
    let r = rig(true);
    let one = HookValue::Int(1);
    r.api
        .get_network_option(&one, "x", HookValue::Bool(false))
        .unwrap();
    assert!(r.notoption(1, "x"));
    assert!(r.api.add_network_option(&one, "x", s("v")).unwrap());
    assert!(!r.notoption(1, "x"));
    r.api
        .get_network_option(&one, "y", HookValue::Bool(false))
        .unwrap();
    assert!(r.notoption(1, "y"));
    assert!(r.api.update_network_option(&one, "y", s("v")).unwrap());
    assert!(!r.notoption(1, "y"));
    assert_eq!(
        r.api
            .get_network_option(&one, "y", HookValue::Bool(false))
            .unwrap(),
        s("v")
    );
}

#[test]
fn delete_sets_notoptions() {
    let r = rig(true);
    let one = HookValue::Int(1);
    assert!(r.api.add_network_option(&one, "x", s("v")).unwrap());
    assert!(r.api.delete_network_option(&one, "x").unwrap());
    assert!(r.notoption(1, "x"));
    assert_eq!(r.cached("1:x"), None);
    assert!(!r.api.delete_network_option(&one, "x").unwrap());
}

#[test]
fn update_same_value_is_false() {
    let r = rig(true);
    let one = HookValue::Int(1);
    let value = HookValue::Array(vec![(
        Key::Str("o".into()),
        HookValue::Array(vec![(Key::Str("a".into()), s("b"))]),
    )]);
    assert!(r.api.add_network_option(&one, "x", value.clone()).unwrap());
    assert!(!r.api.update_network_option(&one, "x", value).unwrap());
}

#[test]
fn hooks_in_order() {
    let r = rig(true);
    let one = HookValue::Int(1);
    r.record(&[
        "pre_add_site_option_x",
        "add_site_option_x",
        "add_site_option",
        "pre_site_option_x",
        "site_option_x",
        "pre_update_site_option_x",
        "update_site_option_x",
        "update_site_option",
        "pre_delete_site_option_x",
        "delete_site_option_x",
        "delete_site_option",
    ]);
    let names =
        |log: &[(String, Vec<HookValue>)], only: &[&str]| -> Vec<(String, Vec<HookValue>)> {
            log.iter()
                .filter(|(n, _)| only.contains(&n.as_str()))
                .cloned()
                .collect()
        };
    let x = s("x");
    assert!(r.api.add_network_option(&one, "x", s("v")).unwrap());
    let add = [
        "pre_add_site_option_x",
        "add_site_option_x",
        "add_site_option",
    ];
    assert_eq!(
        names(&r.logged(), &add),
        vec![
            (
                "pre_add_site_option_x".into(),
                vec![s("v"), x.clone(), one.clone()]
            ),
            (
                "add_site_option_x".into(),
                vec![x.clone(), s("v"), one.clone()]
            ),
            (
                "add_site_option".into(),
                vec![x.clone(), s("v"), one.clone()]
            ),
        ]
    );
    assert!(r.api.update_network_option(&one, "x", s("w")).unwrap());
    let update = [
        "pre_site_option_x",
        "site_option_x",
        "pre_update_site_option_x",
        "update_site_option_x",
        "update_site_option",
    ];
    assert_eq!(
        names(&r.logged(), &update),
        vec![
            (
                "pre_site_option_x".into(),
                vec![
                    HookValue::Bool(false),
                    x.clone(),
                    one.clone(),
                    HookValue::Bool(false)
                ]
            ),
            ("site_option_x".into(), vec![s("v"), x.clone(), one.clone()]),
            (
                "pre_update_site_option_x".into(),
                vec![s("w"), s("v"), x.clone(), one.clone()]
            ),
            (
                "update_site_option_x".into(),
                vec![x.clone(), s("w"), s("v"), one.clone()]
            ),
            (
                "update_site_option".into(),
                vec![x.clone(), s("w"), s("v"), one.clone()]
            ),
        ]
    );
    assert!(r.api.delete_network_option(&one, "x").unwrap());
    let delete = [
        "pre_delete_site_option_x",
        "delete_site_option_x",
        "delete_site_option",
    ];
    assert_eq!(
        names(&r.logged(), &delete),
        vec![
            (
                "pre_delete_site_option_x".into(),
                vec![x.clone(), one.clone()]
            ),
            ("delete_site_option_x".into(), vec![x.clone(), one.clone()]),
            ("delete_site_option".into(), vec![x.clone(), one.clone()]),
        ]
    );
}

#[test]
fn pre_site_option_short_circuits() {
    let r = rig(true);
    r.hooks
        .add_filter(
            &HookName::from("pre_site_option_x"),
            Callback::rust("short", |_: &[HookValue]| {
                Ok(HookValue::Str("short".into()))
            }),
            &HookValue::Int(10),
            1,
        )
        .unwrap();
    assert_eq!(
        r.api
            .get_network_option(&HookValue::Int(1), "x", HookValue::Bool(false))
            .unwrap(),
        s("short")
    );
    assert_eq!(r.sitemeta.calls.get(), 0);
}

#[test]
fn network_id_parameter() {
    let r = rig(true);
    r.sitemeta.seed(1, "x", "one");
    r.sitemeta.seed(2, "x", "two");
    let get = |id: HookValue| {
        r.api
            .get_network_option(&id, "x", HookValue::Bool(false))
            .unwrap()
    };
    for id in [
        HookValue::Int(1),
        s("1"),
        HookValue::Null,
        HookValue::Bool(false),
        HookValue::Int(0),
        s("0"),
    ] {
        assert_eq!(get(id.clone()), s("one"), "{id:?}");
    }
    assert_eq!(get(HookValue::Int(2)), s("two"));
    let calls = r.sitemeta.calls.get();
    r.record(&[
        "pre_site_option_x",
        "pre_site_option",
        "pre_add_site_option_x",
        "pre_delete_site_option_x",
    ]);
    for id in [
        s("abc"),
        HookValue::Array(vec![(Key::Int(0), HookValue::Int(1))]),
    ] {
        assert_eq!(get(id.clone()), HookValue::Bool(false), "{id:?}");
        assert!(!r.api.add_network_option(&id, "x", s("v")).unwrap());
        assert!(!r.api.update_network_option(&id, "x", s("v")).unwrap());
        assert!(!r.api.delete_network_option(&id, "x").unwrap());
        r.api
            .prime_network_option_caches(&id, &["x".into()])
            .unwrap();
    }
    assert!(r.logged().is_empty());
    assert_eq!(r.sitemeta.calls.get(), calls);
}

#[test]
fn prime_skips_cached_and_notoptions() {
    let r = rig(true);
    let one = HookValue::Int(1);
    r.sitemeta.seed(1, "a", "A");
    r.sitemeta.seed(1, "b", "B");
    r.current.current().add_global_groups(&[s("site-options")]);
    r.current
        .current()
        .set(&s("1:c"), s("C"), &s("site-options"), 0)
        .unwrap();
    r.api
        .get_network_option(&one, "d", HookValue::Bool(false))
        .unwrap();
    r.sitemeta.find_many.borrow_mut().clear();
    let names: Vec<String> = ["a", "b", "c", "d", "e"]
        .iter()
        .map(|n| n.to_string())
        .collect();
    r.api.prime_network_option_caches(&one, &names).unwrap();
    assert_eq!(
        *r.sitemeta.find_many.borrow(),
        vec![vec!["a".to_string(), "b".into(), "e".into()]]
    );
    assert_eq!(r.cached("1:a"), Some(s("A")));
    assert!(r.notoption(1, "e"));
}

#[test]
fn prime_single_site_uses_option_prime() {
    let r = rig(false);
    r.repo
        .add(&OptionEntity {
            name: OptionName::new("a").unwrap(),
            value: SerializedValue::new(b"A".to_vec()),
            autoload: Autoload::Off,
        })
        .unwrap();
    r.api
        .prime_network_option_caches(&HookValue::Null, &["a".into()])
        .unwrap();
    assert_eq!(r.sitemeta.calls.get(), 0);
    // Primed: the read after it sends nothing more.
    let sent = r.repo.statements();
    assert_eq!(r.options.get_option("a", None).unwrap(), s("A"));
    assert_eq!(r.repo.statements(), sent);
}

#[test]
fn load_core_site_options_only_on_multisite() {
    let single = rig(false);
    single.api.load_core_site_options(&HookValue::Null).unwrap();
    assert_eq!(single.sitemeta.calls.get(), 0);
    let multi = rig(true);
    multi.api.load_core_site_options(&HookValue::Null).unwrap();
    let primed = multi.sitemeta.find_many.borrow();
    assert_eq!(primed.len(), 1);
    assert_eq!(
        primed[0],
        [
            "site_name",
            "siteurl",
            "active_sitewide_plugins",
            "_site_transient_timeout_theme_roots",
            "_site_transient_theme_roots",
            "site_admins",
            "global_terms_enabled",
            "ms_files_rewriting",
            "WPLANG"
        ]
    );
}

#[test]
fn protected_names_refused() {
    let r = rig(true);
    let err = r
        .api
        .add_network_option(&HookValue::Int(1), "alloptions", s("v"))
        .unwrap_err();
    assert!(matches!(err, OptionsError::Protected(_)), "{err:?}");
    assert_eq!(*r.protect.0.borrow(), vec!["alloptions".to_string()]);
}
