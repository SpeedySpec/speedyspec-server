//! The database the worker's native WordPress areas use, read from the worker's environment.

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_data_mysql::MySqlEngineFactory;
use speedyspec_data_query::{EngineRegistry, Query};
use speedyspec_php::NativeArea;
use speedyspec_wp_cron_php::CronArea;
use speedyspec_wp_db::WpDbConfig;
use speedyspec_wp_db_php::WpDbArea;
use speedyspec_wp_hooks::WpHookApi;
use speedyspec_wp_meta_php::MetaArea;
use speedyspec_wp_object_cache_php::ObjectCacheArea;
use speedyspec_wp_options_php::OptionsArea;
use speedyspec_wp_posts_php::PostsArea;

/// What a worker with a database keeps for its life: the engines, reset at each request end.
pub struct WorkerDatabase {
    pub registry: Rc<EngineRegistry>,
}

/// Why a worker with database credentials cannot boot.
#[derive(Debug)]
pub enum BootFailure {
    /// The configuration was refused (logged as `php_worker_boot_failed`).
    Config(String),
    /// The `default` engine does not answer (logged as `php_worker_database_unreachable`).
    Unreachable(String),
}

impl BootFailure {
    /// The log event and the message.
    pub fn event(&self) -> (&'static str, &str) {
        match self {
            BootFailure::Config(message) => ("php_worker_boot_failed", message),
            BootFailure::Unreachable(message) => ("php_worker_database_unreachable", message),
        }
    }
}

/// A trivial query on the `default` engine, before any area uses it.
fn check(registry: &EngineRegistry) -> Result<(), String> {
    let store = registry.sql("default").map_err(|e| e.to_string())?;
    let statement = Query::parse("SELECT 1")
        .and_then(|q| q.build())
        .map_err(|e| e.to_string())?;
    store
        .query(&statement)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// The `default` engine from `config`, and the areas that use it, in the order they must be registered:
/// `WpDbArea`, `ObjectCacheArea`, `MetaArea`, `PostsArea`, `OptionsArea`, `CronArea`. The engine must answer before `WpDbArea` is built. The
/// object cache has no persistent level yet (request lifetime, as WordPress's default).
pub fn boot_database(
    config: WpDbConfig,
    container: &Rc<Container>,
    hook_api: &Rc<WpHookApi>,
    wake: Rc<crate::wake::NextWake>,
) -> Result<(WorkerDatabase, Vec<Rc<dyn NativeArea>>), BootFailure> {
    let registry = Rc::new(EngineRegistry::new());
    registry.register_factory(Rc::new(MySqlEngineFactory));
    let engine = config
        .engine_config("default")
        .map_err(|e| BootFailure::Config(e.to_string()))?;
    registry
        .configure(engine)
        .map_err(|e| BootFailure::Config(e.to_string()))?;
    check(&registry).map_err(BootFailure::Unreachable)?;
    let wpdb = WpDbArea::new(
        container.clone(),
        hook_api.clone(),
        registry.clone(),
        config,
    );
    let cache = ObjectCacheArea::new(container.clone(), None);
    // The meta area takes the contracts only: the Hook API, the object cache and wpdb as `Rc<dyn …>`.
    let meta = MetaArea::new(container.clone(), hook_api.clone(), cache.api(), wpdb.api());
    let options = OptionsArea::new(container.clone(), hook_api.clone(), cache.api(), wpdb.api());
    // The post registries read PHP's globals live; post reading takes the contracts only, the metadata API and the
    // options API (`is_sticky()`'s `sticky_posts`) among them.
    let posts = PostsArea::new(
        container.clone(),
        hook_api.clone(),
        cache.api(),
        wpdb.api(),
        meta.api(),
        options.api(),
    );
    let cron = CronArea::new(hook_api.clone(), options.api()).with_wake(wake);
    let database = WorkerDatabase { registry };
    let areas: Vec<Rc<dyn NativeArea>> = vec![
        Rc::new(wpdb),
        Rc::new(cache),
        Rc::new(meta),
        Rc::new(posts),
        Rc::new(options),
        Rc::new(cron),
    ];
    Ok((database, areas))
}

/// The worker's database, from `DB_NAME`, `DB_USER`, `DB_PASSWORD`, `DB_HOST`, `DB_CHARSET`, `DB_COLLATE` and
/// `TABLE_PREFIX` (WordPress's `wp-config.php` names). The areas are fixed when PHP boots, so the configuration must
/// exist before any PHP runs; the server already passes each app's credentials through the worker's environment.
/// `None` when `DB_NAME` is unset or empty: the worker then serves without the database areas.
pub fn database_config(env: impl Fn(&str) -> Option<String>) -> Result<Option<WpDbConfig>, String> {
    let name = env("DB_NAME").unwrap_or_default();
    if name.is_empty() {
        return Ok(None);
    }
    let optional = |key: &str| env(key).filter(|value| !value.is_empty());
    let table_prefix = env("TABLE_PREFIX").unwrap_or_else(|| "wp_".to_string());
    // wp-settings.php refuses any other character in $table_prefix.
    if !table_prefix
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(format!(
            "TABLE_PREFIX may contain only letters, digits and underscores: {table_prefix}"
        ));
    }
    Ok(Some(WpDbConfig {
        user: env("DB_USER").unwrap_or_default(),
        password: env("DB_PASSWORD").unwrap_or_default(),
        name,
        host: env("DB_HOST").unwrap_or_else(|| "localhost".to_string()),
        charset: optional("DB_CHARSET"),
        collate: optional("DB_COLLATE"),
        table_prefix,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |key| map.get(key).cloned()
    }

    #[test]
    fn no_db_name_means_no_database() {
        assert_eq!(database_config(env(&[])), Ok(None));
        assert_eq!(database_config(env(&[("DB_NAME", "")])), Ok(None));
    }

    #[test]
    fn defaults_match_wordpress() {
        let config = database_config(env(&[("DB_NAME", "wp")])).unwrap().unwrap();
        assert_eq!(
            config,
            WpDbConfig {
                user: String::new(),
                password: String::new(),
                name: "wp".into(),
                host: "localhost".into(),
                charset: None,
                collate: None,
                table_prefix: "wp_".into(),
            }
        );
    }

    #[test]
    fn host_passes_through_unparsed() {
        let config = database_config(env(&[
            ("DB_NAME", "wp"),
            ("DB_HOST", "localhost:/tmp/mysql.sock"),
            ("DB_CHARSET", "utf8mb4"),
            ("DB_COLLATE", ""),
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(config.host, "localhost:/tmp/mysql.sock");
        assert_eq!(config.charset.as_deref(), Some("utf8mb4"));
        assert_eq!(config.collate, None);
    }

    #[test]
    fn bad_prefix_is_refused() {
        let err = database_config(env(&[("DB_NAME", "wp"), ("TABLE_PREFIX", "wp-")])).unwrap_err();
        assert!(err.contains("wp-"), "{err}");
    }
}
