//! `update_if` and `add_if_absent` on MySQL: the compare is on the stored bytes, not the column's collation.

use std::collections::BTreeMap;
use std::rc::Rc;

use speedyspec_data::EngineConfig;
use speedyspec_data_mysql::MySqlEngineFactory;
use speedyspec_data_query::EngineRegistry;
use speedyspec_wp_db::{MemoryWpdbHost, MemoryWpdbState, WpDb, WpDbConfig, WpDbParts};
use speedyspec_wp_hooks::{HookApi, IgnoreDeprecations};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_options::Option as OptionEntity;
use speedyspec_wp_options::{Autoload, OptionName, OptionRepository, SerializedValue};
use speedyspec_wp_options_sql::SqlOptionRepository;

/// A repository over its own `cascond_options` table, or `None` without `SPEEDYSPEC_TEST_MYSQL_URL`.
fn repo() -> Option<SqlOptionRepository> {
    let url = std::env::var("SPEEDYSPEC_TEST_MYSQL_URL").ok()?;
    let database = url.rsplit('/').next().unwrap_or_default().to_string();
    let registry = EngineRegistry::new();
    registry.register_factory(Rc::new(MySqlEngineFactory));
    registry
        .configure(EngineConfig {
            name: "default".into(),
            url,
            options: BTreeMap::new(),
        })
        .unwrap();
    let db = Rc::new(WpDb::new(WpDbParts {
        store: registry.sql("default").unwrap(),
        hooks: Rc::new(HookApi::new(
            Rc::new(MemoryHookStore::new()),
            Rc::new(IgnoreDeprecations),
        )),
        state: Rc::new(MemoryWpdbState::new()),
        host: Rc::new(MemoryWpdbHost::new()),
        config: WpDbConfig {
            user: String::new(),
            password: String::new(),
            name: database,
            host: "localhost".into(),
            charset: Some("utf8mb4".into()),
            collate: None,
            table_prefix: "cascond_".into(),
        },
    }));
    db.db_connect(true).unwrap();
    db.set_prefix("cascond_", true).unwrap();
    db.query("DROP TABLE IF EXISTS cascond_options").unwrap();
    db.query(
        "CREATE TABLE cascond_options (option_id bigint(20) unsigned NOT NULL auto_increment, \
         option_name varchar(191) NOT NULL default '', option_value longtext NOT NULL, \
         autoload varchar(20) NOT NULL default 'yes', PRIMARY KEY (option_id), UNIQUE KEY option_name (option_name), \
         KEY autoload (autoload)) DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_520_ci",
    )
    .unwrap();
    Some(SqlOptionRepository::new(db, "default".into()))
}

fn bytes(v: &str) -> SerializedValue {
    SerializedValue::new(v.as_bytes().to_vec())
}

fn name() -> OptionName {
    OptionName::new("cron").unwrap()
}

#[test]
fn binary_compare_refuses_case_variants() {
    let Some(repo) = repo() else {
        println!("skip: SPEEDYSPEC_TEST_MYSQL_URL is not set");
        return;
    };
    repo.add(&OptionEntity {
        name: name(),
        value: bytes(r#"s:3:"abc";"#),
        autoload: Autoload::On,
    })
    .unwrap();
    assert_eq!(
        repo.update_if(&name(), &bytes(r#"s:3:"ABC";"#), Some(&bytes("x")), None)
            .unwrap(),
        0
    );
    assert_eq!(
        repo.value(&name(), true).unwrap().unwrap().as_bytes(),
        br#"s:3:"abc";"#
    );
    assert_eq!(
        repo.update_if(&name(), &bytes(r#"s:3:"abc";"#), Some(&bytes("x")), None)
            .unwrap(),
        1
    );
    assert_eq!(repo.value(&name(), true).unwrap().unwrap().as_bytes(), b"x");
    let again = OptionEntity {
        name: name(),
        value: bytes("y"),
        autoload: Autoload::On,
    };
    assert_eq!(repo.add_if_absent(&again).unwrap(), 0);
    assert_eq!(repo.value(&name(), true).unwrap().unwrap().as_bytes(), b"x");
}
