//! WordPress's `wpdb` class and the `$wpdb` global in PHP, native over the Rust `wpdb` port (Facade, Gateway).
//!
//! [`WpDbArea`] declares, at the start of every request (from `wpdb.php`), the constants `wpdb` defines and the
//! `wpdb` class with every method of WordPress's, each one line calling a native function under
//! `SpeedySpec\WpDb\Native\` with the instance's handle. Handle 0 is the global `$wpdb`, made at request start over
//! the worker's `default` engine; its calls go through the handlers bound in the container (resolved once), so PHP
//! may override one. Another `new wpdb(...)` gets its own `WpDb` and request state on an engine configured once per
//! worker for its credentials.
//!
//! The facade converts and holds no rules: rows come back as arrays and PHP turns them into `stdClass` objects,
//! kept per result generation so `get_results()` and `$wpdb->last_result` hand out the same objects.
//!
//! **Documented breaks:** `$wpdb->dbh` is a `SpeedySpec\WpDb\Connection` object, not a `mysqli` (`dbh_is_not_mysqli`);
//! the placeholder escape is removed in Rust, not by a `query` callback (`placeholder_escape_filter_not_registered`);
//! unsetting a declared public property throws (`unset_public_property`).

mod convert;
mod host;
mod natives;
mod state;

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use sha2::{Digest, Sha256};
use speedyspec_container::Container;
use speedyspec_data::{EngineLookup, SqlStore};
use speedyspec_data_query::EngineRegistry;
use speedyspec_php::NativeArea;
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_php_ext::Value;
use speedyspec_wp_db::{
    MemoryWpdbState, WPDB_STATE, WpDb, WpDbConfig, WpDbError, WpDbParts, WpdbState,
    register_wpdb_handlers,
};
use speedyspec_wp_hooks::HookApi;

pub use host::PhpWpdbHost;
pub use state::{PhpWpdbState, ResolvedWpdbState};

const FACADE: &str = include_str!("wpdb.php");

pub(crate) struct Instances {
    pub extra: Vec<Option<Rc<WpDb>>>,
}

pub(crate) struct Shared {
    pub hooks: Rc<HookApi>,
    pub registry: Rc<EngineRegistry>,
    pub global: Rc<WpDb>,
    pub memory: Rc<MemoryWpdbState>,
    pub handlers: natives::Handlers,
}

thread_local! {
    pub(crate) static SHARED: RefCell<Option<Rc<Shared>>> = const { RefCell::new(None) };
    pub(crate) static INSTANCES: RefCell<Instances> = const { RefCell::new(Instances { extra: Vec::new() }) };
}

/// The native area of WordPress's `wpdb`.
pub struct WpDbArea {
    shared: Rc<Shared>,
    global: bool,
}

impl WpDbArea {
    /// The global `WpDb` over `registry.sql("default")`, its request state bound under [`WPDB_STATE`] (the worker's
    /// memory state, which PHP may replace), the PHP host, and every handler registered in `container`.
    pub fn new(
        container: Rc<Container>,
        hooks: Rc<HookApi>,
        registry: Rc<EngineRegistry>,
        config: WpDbConfig,
    ) -> WpDbArea {
        let store: Rc<dyn SqlStore> = match registry.sql("default") {
            Ok(store) => store,
            Err(e) => panic!("the default engine does not open: {e}"),
        };
        let memory = Rc::new(MemoryWpdbState::new());
        let bound: Rc<dyn WpdbState> = memory.clone();
        container.singleton_port::<dyn WpdbState>(WPDB_STATE, move |_, _| Ok(bound.clone()));
        let state: Rc<dyn WpdbState> =
            Rc::new(ResolvedWpdbState::new(container.clone(), memory.clone()));
        let global = Rc::new(WpDb::new(WpDbParts {
            store,
            hooks: hooks.clone(),
            state,
            host: Rc::new(PhpWpdbHost),
            config,
        }));
        register_wpdb_handlers(&container, global.clone());
        let handlers = natives::Handlers::new(&container);
        let shared = Rc::new(Shared {
            hooks,
            registry,
            global,
            memory,
            handlers,
        });
        SHARED.with(|s| *s.borrow_mut() = Some(shared.clone()));
        WpDbArea {
            shared,
            global: true,
        }
    }

    /// No `$wpdb` at request start, for hosts that make their own.
    pub fn without_global(mut self) -> WpDbArea {
        self.global = false;
        self
    }

    /// The global `wpdb` for Rust callers.
    pub fn api(&self) -> Rc<WpDb> {
        self.shared.global.clone()
    }

    /// Refuses a `wp-content/db.php` drop-in: the native class replaces it.
    pub fn check_drop_in(wp_content_dir: &Path) -> Result<(), WpDbError> {
        let path = wp_content_dir.join("db.php");
        if path.exists() {
            return Err(WpDbError::DropInRefused {
                path: path.display().to_string(),
            });
        }
        Ok(())
    }
}

/// A `new wpdb(...)` other than the global: its own `WpDb` and state on an engine for its credentials.
pub(crate) fn open(user: &str, password: &str, name: &str, host: &str) -> Result<i64, WpDbError> {
    let shared = SHARED
        .with(|s| s.borrow().clone())
        .ok_or(WpDbError::NotReady)?;
    let config = WpDbConfig {
        user: user.into(),
        password: password.into(),
        name: name.into(),
        host: host.into(),
        charset: shared.global.config().charset.clone(),
        collate: shared.global.config().collate.clone(),
        table_prefix: shared.global.config().table_prefix.clone(),
    };
    let probe = config.engine_config("probe")?;
    let digest = Sha256::digest(probe.url.as_bytes());
    let engine = format!(
        "wpdb-{}",
        digest
            .iter()
            .take(8)
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    );
    if !shared.registry.commit_order().contains(&engine) {
        shared.registry.configure(config.engine_config(&engine)?)?;
    }
    let store = shared
        .registry
        .handle(&engine)?
        .as_sql()
        .cloned()
        .ok_or_else(|| WpDbError::Config(format!("engine `{engine}` is not a SQL store")))?;
    let db = Rc::new(WpDb::new(WpDbParts {
        store,
        hooks: shared.hooks.clone(),
        state: Rc::new(MemoryWpdbState::new()),
        host: Rc::new(PhpWpdbHost),
        config,
    }));
    Ok(INSTANCES.with(|i| {
        let mut i = i.borrow_mut();
        i.extra.push(Some(db));
        i.extra.len() as i64
    }))
}

/// The instance behind a handle (0 is the global).
pub(crate) fn instance(handle: i64) -> Option<(Rc<WpDb>, bool)> {
    if handle == 0 {
        return SHARED.with(|s| s.borrow().as_ref().map(|s| (s.global.clone(), true)));
    }
    INSTANCES.with(|i| {
        i.borrow()
            .extra
            .get(usize::try_from(handle - 1).ok()?)
            .cloned()
            .flatten()
            .map(|db| (db, false))
    })
}

impl NativeArea for WpDbArea {
    fn functions(&self) -> Vec<FunctionBuilder<'static>> {
        natives::functions()
    }

    fn request_start(&self) {
        self.shared.memory.reset();
        self.shared.global.state().reset();
        self.shared.global.state().modify(&mut |s| {
            s.ready = true;
            s.dbh_present = true;
        });
        let installed = speedyspec_php::eval("speedyspec://wp-db", FACADE).and_then(|_| {
            if self.global {
                speedyspec_php::eval(
                    "speedyspec://wp-db-global",
                    "$GLOBALS['wpdb'] = (new \\ReflectionClass('wpdb'))->newInstanceWithoutConstructor(); return true;",
                )
            } else {
                Ok(Value::Null)
            }
        });
        if let Err(err) = installed {
            eprintln!("speedyspec_wordpress: installing wpdb failed: {err}");
            return;
        }
        if self.global
            && let Err(err) = self.shared.global.db_connect(true)
        {
            eprintln!("speedyspec_wordpress: connecting wpdb failed: {err}");
        }
    }

    fn request_end(&self) {
        INSTANCES.with(|i| i.borrow_mut().extra.clear());
    }
}
