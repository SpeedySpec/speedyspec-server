//! `SqlPostRepository` over `NativeWpdb` on every engine: the conformance suite, MySQL's collation and zero dates, the
//! switched prefix, hook failures, and the statement text the `query` filter sees. MySQL and PostgreSQL run when
//! `SPEEDYSPEC_TEST_MYSQL_URL` or `SPEEDYSPEC_TEST_POSTGRES_URL` is set, and print `skipped` otherwise.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Mutex;

use serde_json::Value;
use speedyspec_data::{EngineConfig, EngineFactory};
use speedyspec_data_mysql::MySqlEngineFactory;
use speedyspec_data_postgres::PostgresEngineFactory;
use speedyspec_data_query::EngineRegistry;
use speedyspec_data_sqlite::SqliteEngineFactory;
use speedyspec_wp_db::{MemoryWpdbHost, MemoryWpdbState, NativeWpdb, WpDbConfig, WpDbParts};
use speedyspec_wp_db_contracts::{WpValue, take_hook_error};
use speedyspec_wp_hooks::{IgnoreDeprecations, WpHookApi};
use speedyspec_wp_hooks_contracts::{Callback, HookError, HookName};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_posts_contracts::conformance::{mysql_suite, post_repository_suite, read_suite};
use speedyspec_wp_posts_contracts::{
    LastPostField, MimeFilter, Post, PostDate, PostDateTime, PostError, PostId, PostParent,
    PostRepository, PostStatusName, PostTypeName, Slug, Timezone,
};
use speedyspec_wp_posts_sql::SqlPostRepository;

/// The tests share the posts tables: one at a time.
static TABLES: Mutex<()> = Mutex::new(());

/// The engines the repository must run on.
#[derive(Clone, Copy)]
enum Engine {
    MySql,
    Sqlite,
    Postgres,
}

impl Engine {
    fn name(self) -> &'static str {
        match self {
            Engine::MySql => "mysql",
            Engine::Sqlite => "sqlite",
            Engine::Postgres => "postgres",
        }
    }
}

/// The PostgreSQL schema these tests own, dropped and made afresh for each test.
const PG_SCHEMA: &str = "t_posts_engines";

/// `wp_posts`, as `wp-admin/includes/schema.php` creates it (WordPress 7.2-RC2).
fn posts_table(table: &str) -> String {
    format!(
        "CREATE TABLE {table} (ID bigint(20) unsigned NOT NULL auto_increment, \
         post_author bigint(20) unsigned NOT NULL default '0', \
         post_date datetime NOT NULL default '0000-00-00 00:00:00', \
         post_date_gmt datetime NOT NULL default '0000-00-00 00:00:00', \
         post_content longtext NOT NULL, post_title text NOT NULL, post_excerpt text NOT NULL, \
         post_status varchar(20) NOT NULL default 'publish', comment_status varchar(20) NOT NULL default 'open', \
         ping_status varchar(20) NOT NULL default 'open', post_password varchar(255) NOT NULL default '', \
         post_name varchar(200) NOT NULL default '', to_ping text NOT NULL, pinged text NOT NULL, \
         post_modified datetime NOT NULL default '0000-00-00 00:00:00', \
         post_modified_gmt datetime NOT NULL default '0000-00-00 00:00:00', \
         post_content_filtered longtext NOT NULL, post_parent bigint(20) unsigned NOT NULL default '0', \
         guid varchar(255) NOT NULL default '', menu_order int(11) NOT NULL default '0', \
         post_type varchar(20) NOT NULL default 'post', post_mime_type varchar(100) NOT NULL default '', \
         comment_count bigint(20) NOT NULL default '0', PRIMARY KEY (ID), KEY post_name (post_name(191)), \
         KEY type_status_date (post_type,post_status,post_date,ID), KEY post_parent (post_parent), \
         KEY post_author (post_author)) DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_520_ci"
    )
}

struct Rig {
    db: Rc<NativeWpdb>,
    hooks: Rc<WpHookApi>,
    host: Rc<MemoryWpdbHost>,
    repo: SqlPostRepository,
}

/// `NativeWpdb` over `engine` with fresh `wp_posts` and `wp_2_posts` tables; `None` when the engine's URL is unset.
fn rig(engine: Engine) -> Option<Rig> {
    let (factory, url, options): (Rc<dyn EngineFactory>, String, BTreeMap<String, String>) =
        match engine {
            Engine::MySql => (
                Rc::new(MySqlEngineFactory),
                std::env::var("SPEEDYSPEC_TEST_MYSQL_URL").ok()?,
                BTreeMap::new(),
            ),
            Engine::Sqlite => (
                Rc::new(SqliteEngineFactory),
                "sqlite://:memory:".into(),
                BTreeMap::from([("database".into(), "speedyspec_test".into())]),
            ),
            Engine::Postgres => {
                let url = std::env::var("SPEEDYSPEC_TEST_POSTGRES_URL").ok()?;
                let mut admin =
                    postgres::Client::connect(&url, postgres::NoTls).expect("PostgreSQL is up");
                admin
                    .batch_execute(&format!(
                        "DROP SCHEMA IF EXISTS {PG_SCHEMA} CASCADE; CREATE SCHEMA {PG_SCHEMA}"
                    ))
                    .unwrap();
                (
                    Rc::new(PostgresEngineFactory),
                    url,
                    BTreeMap::from([("schema".into(), PG_SCHEMA.into())]),
                )
            }
        };
    let database = match engine {
        Engine::MySql => url.rsplit('/').next().unwrap_or_default().to_string(),
        _ => "speedyspec_test".to_string(),
    };
    let registry = EngineRegistry::new();
    registry.register_factory(factory);
    registry
        .configure(EngineConfig {
            name: "default".into(),
            url,
            options,
        })
        .unwrap();
    let hooks = Rc::new(WpHookApi::new(
        Rc::new(MemoryHookStore::new()),
        Rc::new(IgnoreDeprecations),
    ));
    let host = Rc::new(MemoryWpdbHost::new());
    let db = Rc::new(NativeWpdb::new(WpDbParts {
        store: registry.sql("default").unwrap(),
        hooks: hooks.clone(),
        state: Rc::new(MemoryWpdbState::new()),
        host: host.clone(),
        config: WpDbConfig {
            user: String::new(),
            password: String::new(),
            name: database,
            host: "localhost".into(),
            charset: Some("utf8mb4".into()),
            collate: None,
            table_prefix: "wp_".into(),
        },
    }));
    // `db_connect` runs `wpdb::set_sql_mode`, so every statement below runs under WordPress's mode.
    db.db_connect(true).unwrap();
    db.set_prefix("wp_", true).unwrap();
    for table in ["wp_posts", "wp_2_posts"] {
        db.query(&format!("DROP TABLE IF EXISTS {table}")).unwrap();
        db.query(&posts_table(table)).unwrap();
    }
    let repo = SqlPostRepository::new(db.clone(), engine.name());
    Some(Rig {
        db,
        hooks,
        host,
        repo,
    })
}

fn count(db: &NativeWpdb, table: &str) -> u64 {
    db.query(&format!("SELECT COUNT(*) FROM {table}")).unwrap();
    db.get_col(0)
        .into_iter()
        .flatten()
        .next()
        .and_then(|b| String::from_utf8(b).ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

/// Runs `test` on `engine`, or prints `skipped` when its URL is unset.
fn on(engine: Engine, test: impl FnOnce(Rig)) {
    let _one = TABLES.lock().unwrap_or_else(|e| e.into_inner());
    match rig(engine) {
        Some(rig) => test(rig),
        None => println!("skipped: no {} URL is set", engine.name()),
    }
}

fn named(name: &str, kind: &str) -> Post {
    Post {
        name: Slug::from(name),
        post_type: PostTypeName::new(kind),
        status: PostStatusName::new("publish"),
        ..Post::default()
    }
}

fn suite(engine: Engine) {
    on(engine, |rig| {
        assert_eq!(rig.repo.engine(), engine.name());
        post_repository_suite(&rig.repo);
    });
}

#[test]
fn suite_on_mysql() {
    suite(Engine::MySql);
}

/// The read checks only: the write checks are MySQL's answers under `wpdb::set_sql_mode()`'s mode, which the SQLite
/// engine does not copy. It stores a `-5` parent as `-5` and rejects the `NULL`-parent update that MySQL stores as 0.
#[test]
fn suite_on_sqlite() {
    on(Engine::Sqlite, |rig| {
        assert_eq!(rig.repo.engine(), Engine::Sqlite.name());
        read_suite(&rig.repo);
    });
}

/// Not run locally or in CI yet. Expect the suite's NUL-content post (`find_returns_what_insert_wrote`) to fail
/// here: PostgreSQL's `text` cannot hold a `0x00` byte, which MySQL's `longtext` (and SQLite) store.
#[test]
fn suite_on_postgres() {
    suite(Engine::Postgres);
}

/// A NUL byte in a column (mysqli's `\0` escape) is stored and read back whole.
fn nul_content_round_trips(engine: Engine) {
    on(engine, |rig| {
        let post = Post {
            content: b"before\x00after".to_vec(),
            ..named("nul", "post")
        };
        let id = rig.repo.insert(&post, None).unwrap().expect("an id");
        assert_eq!(rig.repo.find(id).unwrap(), Some(Post { id, ..post }));
    });
}

#[test]
fn nul_content_round_trips_on_sqlite() {
    nul_content_round_trips(Engine::Sqlite);
}

#[test]
fn nul_content_round_trips_on_mysql() {
    nul_content_round_trips(Engine::MySql);
}

/// `_get_last_post_time()` answers nothing for only drafts, an unknown type, or no types (`IN ('')`): the 0.3.0
/// suite's `last_post_time_of_nothing_is_none`, which the MySQL known break skips.
fn last_post_time_of_nothing_is_none(engine: Engine) {
    on(engine, |rig| {
        let at = PostDateTime::new("2024-01-01 00:00:00");
        rig.repo
            .insert(
                &Post {
                    status: PostStatusName::new("draft"),
                    date: PostDate {
                        local: at.clone(),
                        gmt: at,
                    },
                    ..named("dated", "rd-last-draft")
                },
                None,
            )
            .unwrap()
            .expect("an id");
        let last = |kinds: &[&str], field, timezone| {
            let kinds: Vec<_> = kinds.iter().map(|k| PostTypeName::new(*k)).collect();
            rig.repo.last_post_time(field, &kinds, timezone).unwrap()
        };
        assert_eq!(
            last(&["rd-last-draft"], LastPostField::Date, Timezone::Blog),
            None
        );
        assert_eq!(last(&["rd-none"], LastPostField::Date, Timezone::Gmt), None);
        assert_eq!(last(&[], LastPostField::Modified, Timezone::Blog), None);
        assert_eq!(
            last(&["rd-last-draft"], LastPostField::Date, Timezone::Server),
            None
        );
    });
}

#[test]
fn last_post_time_of_nothing_is_none_on_mysql() {
    last_post_time_of_nothing_is_none(Engine::MySql);
}

#[test]
fn last_post_time_of_nothing_is_none_on_sqlite() {
    last_post_time_of_nothing_is_none(Engine::Sqlite);
}

/// `get_lastpostdate( 'server' )` on MySQL: `DATE_ADD` answers with six fractional digits, and the repository
/// returns them as WordPress does.
#[test]
fn server_time_keeps_mysql_fraction() {
    on(Engine::MySql, |rig| {
        let at = PostDateTime::new("2024-03-01 20:00:00");
        rig.repo
            .insert(
                &Post {
                    date: PostDate {
                        local: at.clone(),
                        gmt: at,
                    },
                    ..named("dated", "post")
                },
                None,
            )
            .unwrap()
            .expect("an id");
        let server = rig
            .repo
            .last_post_time(
                LastPostField::Date,
                &[PostTypeName::new("post")],
                Timezone::Server,
            )
            .unwrap();
        assert_eq!(
            server,
            Some(PostDateTime::new("2024-03-01 20:00:00.000000"))
        );
    });
}

/// `utf8mb4_unicode_520_ci` and `DATE_ADD` on a zero date, as MySQL answers them.
#[test]
fn mysql_suite_on_mysql() {
    on(Engine::MySql, |rig| mysql_suite(&rig.repo));
}

/// `0000-00-00 00:00:00` is written and read back unchanged under `wpdb::set_sql_mode`'s mode, by `find`,
/// `find_many` and `last_post_time`.
fn zero_dates_round_trip(engine: Engine) {
    on(engine, |rig| {
        let zero = PostDate {
            local: PostDateTime::ZERO,
            gmt: PostDateTime::ZERO,
        };
        let post = Post {
            date: zero.clone(),
            modified: zero,
            ..named("zero", "post")
        };
        let id = rig.repo.insert(&post, None).unwrap().expect("an id");
        let expected = Post { id, ..post };
        assert_eq!(rig.repo.find(id).unwrap(), Some(expected.clone()));
        assert_eq!(rig.repo.find_many(&[id]).unwrap(), vec![expected]);
        for field in [LastPostField::Date, LastPostField::Modified] {
            for timezone in [Timezone::Blog, Timezone::Gmt] {
                let last = rig
                    .repo
                    .last_post_time(field, &[PostTypeName::new("post")], timezone)
                    .unwrap();
                assert_eq!(last, Some(PostDateTime::ZERO), "{field:?} {timezone:?}");
            }
        }
        rig.db
            .query(&format!(
                "SELECT post_date_gmt FROM wp_posts WHERE ID = {}",
                id.0
            ))
            .unwrap();
        let stored = rig.db.get_col(0).into_iter().flatten().next();
        assert_eq!(
            stored.as_deref(),
            Some(PostDateTime::ZERO.as_str().as_bytes())
        );
    });
}

#[test]
fn zero_dates_round_trip_on_mysql() {
    zero_dates_round_trip(Engine::MySql);
}

#[test]
fn zero_dates_round_trip_on_sqlite() {
    zero_dates_round_trip(Engine::Sqlite);
}

/// The SQL-level half of `get_page_by_path()`'s case rule: under `utf8mb4_unicode_520_ci` the `IN (...)` lookup
/// matches whatever the case, and its pad-space comparison ignores trailing spaces. (WordPress's own case rule is
/// `sanitize_title()` plus PHP's `===`, the use case's to copy; this checks only what the statement returns.)
#[test]
fn page_by_path_ignores_case() {
    on(Engine::MySql, |rig| {
        let about = rig
            .repo
            .insert(&named("about", "page"), None)
            .unwrap()
            .expect("an id");
        for asked in ["About", "ABOUT", "about "] {
            let rows = rig
                .repo
                .pages_by_names(
                    &[Slug::from(asked)],
                    &[PostTypeName::new("page"), PostTypeName::new("attachment")],
                )
                .unwrap();
            let found: Vec<(PostId, Slug)> = rows.into_iter().map(|r| (r.id, r.name)).collect();
            assert_eq!(found, vec![(about, Slug::from("about"))], "{asked:?}");
        }
    });
}

/// On multisite, after `switch_to_blog( 2 )` (`$wpdb->set_blog_id( 2 )`), every call reads and writes `wp_2_posts`;
/// back on blog 1 it reads `wp_posts` again.
fn follows_switched_prefix(engine: Engine) {
    on(engine, |rig| {
        let main = rig
            .repo
            .insert(&named("main", "page"), None)
            .unwrap()
            .expect("an id");
        rig.host.multisite.replace(true);
        rig.host.define("MULTISITE", WpValue::Bool(true));
        rig.db.set_prefix("wp_", true).unwrap();
        rig.db.set_blog_id(2, 1);
        assert_eq!(rig.repo.find(main).unwrap(), None);
        assert!(rig.repo.all_page_ids().unwrap().is_empty());
        let id = rig
            .repo
            .insert(&named("blog-two", "page"), None)
            .unwrap()
            .expect("an id");
        assert_eq!(count(&rig.db, "wp_2_posts"), 1);
        assert_eq!(count(&rig.db, "wp_posts"), 1);
        assert_eq!(
            rig.repo.find(id).unwrap().map(|p| p.name),
            Some(Slug::from("blog-two"))
        );
        assert_eq!(rig.repo.all_page_ids().unwrap(), vec![id]);
        let rows = rig
            .repo
            .pages_by_names(&[Slug::from("blog-two")], &[PostTypeName::new("page")])
            .unwrap();
        assert_eq!(rows.iter().map(|r| r.id).collect::<Vec<_>>(), vec![id]);
        rig.db.set_blog_id(1, 1);
        assert_eq!(rig.repo.all_page_ids().unwrap(), vec![main]);
    });
}

#[test]
fn follows_switched_prefix_on_sqlite() {
    follows_switched_prefix(Engine::Sqlite);
}

#[test]
fn follows_switched_prefix_on_mysql() {
    follows_switched_prefix(Engine::MySql);
}

#[test]
fn follows_switched_prefix_on_postgres() {
    follows_switched_prefix(Engine::Postgres);
}

/// A `query` callback's failure is `PostError::Hook` with its message, from a read and from a write whose charset
/// lookup `$wpdb` hid it behind; the payload stays for the bridge to take.
#[test]
fn hook_failure_surfaces() {
    on(Engine::Sqlite, |rig| {
        let _ = take_hook_error();
        rig.hooks
            .add_filter(
                &HookName::from("query"),
                Callback::rust("throws", |_: &[WpValue]| Err(HookError::message("boom"))),
                &WpValue::Int(10),
                1,
            )
            .unwrap();
        let failed = rig.repo.find(PostId(1));
        assert!(
            matches!(failed, Err(PostError::Hook(ref e)) if e.message == "boom"),
            "{failed:?}"
        );
        assert_eq!(take_hook_error().map(|e| e.message), Some("boom".into()));
        let hidden = rig.repo.insert(&named("x", "post"), None);
        assert!(matches!(hidden, Err(PostError::Hook(_))), "{hidden:?}");
        assert!(take_hook_error().is_some());
        let none = rig.repo.all_page_ids();
        assert!(matches!(none, Err(PostError::Hook(_))), "{none:?}");
        assert!(take_hook_error().is_some());
    });
}

/// The statement texts `compat/posts-read` recorded from WordPress 7.2-RC2's `query` filter.
struct Recorded(Value);

impl Recorded {
    fn load() -> Recorded {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../compat/posts-read/wordpress-7.2-RC2.json");
        Recorded(serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap())
    }

    /// The query the `rec:query` fixture received in `scenario`'s `step`, at `invocation`.
    fn query(&self, scenario: &str, step: usize, invocation: usize) -> String {
        let call = &self.0["results"][scenario][step]["invocations"][invocation];
        assert_eq!(
            call["fixture"], "rec:query",
            "{scenario} {step} {invocation}"
        );
        call["args"]["array"][0][1]
            .as_str()
            .unwrap_or_else(|| panic!("{scenario} {step} {invocation} recorded a query"))
            .to_string()
    }
}

/// Every method's statement, as the `query` filter sees it, is `post.php`'s: compared with `compat/posts-read`'s
/// recording where a scenario recorded it, and with the contract's documented text otherwise.
#[test]
fn statements_match_post_php() {
    on(Engine::Sqlite, |rig| {
        let sent: Rc<RefCell<Vec<String>>> = Rc::default();
        let log = sent.clone();
        rig.hooks
            .add_filter(
                &HookName::from("query"),
                Callback::rust("record", move |args: &[WpValue]| {
                    let query = args.first().cloned().unwrap_or(WpValue::Null);
                    match &query {
                        WpValue::Str(text) => log.borrow_mut().push(text.clone()),
                        WpValue::Bytes(bytes) => log
                            .borrow_mut()
                            .push(String::from_utf8_lossy(bytes).into_owned()),
                        other => panic!("the query filter received {other:?}"),
                    }
                    Ok(query)
                }),
                &WpValue::Int(10),
                1,
            )
            .unwrap();
        // What one call sent, leaving out `wpdb`'s own charset lookups.
        let take = || -> Vec<String> {
            sent.borrow_mut()
                .drain(..)
                .filter(|q| !q.starts_with("SHOW "))
                .collect()
        };
        // `wp_set_wpdb_vars()`'s `$wpdb->field_types` for the posts table's columns.
        rig.db.set_field_types(
            [
                "post_author",
                "post_parent",
                "menu_order",
                "ID",
                "comment_count",
            ]
            .into_iter()
            .map(|column| (column.to_string(), "%d".to_string()))
            .collect(),
        );
        let recorded = Recorded::load();
        let repo = &rig.repo;
        let types = |names: &[&str]| {
            names
                .iter()
                .map(|n| PostTypeName::new(*n))
                .collect::<Vec<_>>()
        };
        let slugs = |names: &[&str]| names.iter().map(|n| Slug::from(*n)).collect::<Vec<_>>();

        // `get_page_by_path( 'About/team' )`, then `WP_Post::get_instance( 3 )`.
        repo.pages_by_names(&slugs(&["about", "team"]), &types(&["page", "attachment"]))
            .unwrap();
        repo.find(PostId(3)).unwrap();
        assert_eq!(
            take(),
            [
                recorded.query("page-path-statement", 1, 0),
                recorded.query("page-path-statement", 1, 1)
            ]
        );
        // `get_page_by_path( 'about', OBJECT, array( 'book', 'page' ) )`.
        repo.pages_by_names(&slugs(&["about"]), &types(&["book", "page"]))
            .unwrap();
        repo.find(PostId(2)).unwrap();
        assert_eq!(
            take(),
            [
                recorded.query("page-path-statement", 2, 0),
                recorded.query("page-path-statement", 2, 1)
            ]
        );

        // `wp_count_posts( 'post', 'readable' )` as user 2, then `wp_count_posts( 'page' )`.
        repo.count_by_status(&PostTypeName::new("post"), Some(2))
            .unwrap();
        assert_eq!(take(), [recorded.query("count-readable-statement", 2, 0)]);
        repo.count_by_status(&PostTypeName::new("page"), None)
            .unwrap();
        assert_eq!(take(), [recorded.query("count-readable-statement", 3, 0)]);

        // `wp_count_attachments( 'image' )`.
        repo.count_attachments(Some(&MimeFilter::One("image".into())))
            .unwrap();
        assert_eq!(
            take(),
            [
                recorded.query("count-attachments-statement", 1, 0),
                recorded.query("count-attachments-statement", 1, 1)
            ]
        );

        // `get_lastpostdate( 'server' )` (the public types), then `get_lastpostmodified( 'gmt', 'post' )`, which
        // also reads the last post date.
        repo.last_post_time(
            LastPostField::Date,
            &types(&["post", "page", "attachment"]),
            Timezone::Server,
        )
        .unwrap();
        assert_eq!(take(), [recorded.query("last-statement", 1, 0)]);
        repo.last_post_time(LastPostField::Modified, &types(&["post"]), Timezone::Gmt)
            .unwrap();
        repo.last_post_time(LastPostField::Date, &types(&["post"]), Timezone::Gmt)
            .unwrap();
        assert_eq!(
            take(),
            [
                recorded.query("last-statement", 2, 0),
                recorded.query("last-statement", 2, 1)
            ]
        );

        // `_prime_post_caches( array( 3, 1, 2 ) )`.
        repo.find_many(&[PostId(3), PostId(1), PostId(2)]).unwrap();
        assert_eq!(take(), [recorded.query("cache-prime-statement", 1, 0)]);

        // No scenario recorded these; the contract documents WordPress 7.2-RC2's text.
        repo.parent_ids(&[PostId(3), PostId(1)]).unwrap();
        repo.all_page_ids().unwrap();
        repo.available_mime_types(&PostTypeName::new("attachment"))
            .unwrap();
        repo.last_post_time(LastPostField::Modified, &[], Timezone::Blog)
            .unwrap();
        repo.count_attachments(None).unwrap();
        repo.count_attachments(Some(&MimeFilter::Many(vec![
            "image/png".into(),
            "video/**".into(),
            "audio/".into(),
        ])))
        .unwrap();
        repo.count_attachments(Some(&MimeFilter::One("image, %/%".into())))
            .unwrap();
        repo.attachment_slug_owner(&Slug::from("it's"), PostId(4))
            .unwrap();
        repo.hierarchical_slug_owner(
            &Slug::from("child"),
            &PostTypeName::new("page"),
            PostId(4),
            PostParent::Int(2),
        )
        .unwrap();
        repo.flat_slug_owner(&Slug::from("hello"), &PostTypeName::new("post"), PostId(4))
            .unwrap();
        assert_eq!(
            take(),
            [
                "SELECT wp_posts.ID, wp_posts.post_parent FROM wp_posts WHERE ID IN (3,1)",
                "SELECT ID FROM wp_posts WHERE post_type = 'page'",
                "SELECT DISTINCT post_mime_type FROM wp_posts WHERE post_type = 'attachment' AND post_mime_type != ''",
                "SELECT post_modified FROM wp_posts WHERE post_status = 'publish' AND post_type IN ('') \
                 ORDER BY post_modified_gmt DESC LIMIT 1",
                "SELECT post_mime_type, COUNT( * ) AS num_posts FROM wp_posts WHERE post_type = 'attachment' \
                 AND post_status != 'trash'  GROUP BY post_mime_type",
                "SELECT COUNT( * ) FROM wp_posts WHERE post_type = 'attachment' AND post_status = 'trash' ",
                "SELECT post_mime_type, COUNT( * ) AS num_posts FROM wp_posts WHERE post_type = 'attachment' \
                 AND post_status != 'trash'  AND (post_mime_type = 'image/png' OR post_mime_type LIKE 'video/%' \
                 OR post_mime_type LIKE 'audio/%')  GROUP BY post_mime_type",
                "SELECT COUNT( * ) FROM wp_posts WHERE post_type = 'attachment' AND post_status = 'trash'  AND \
                 (post_mime_type = 'image/png' OR post_mime_type LIKE 'video/%' OR post_mime_type LIKE 'audio/%') ",
                "SELECT post_mime_type, COUNT( * ) AS num_posts FROM wp_posts WHERE post_type = 'attachment' \
                 AND post_status != 'trash'  GROUP BY post_mime_type",
                "SELECT COUNT( * ) FROM wp_posts WHERE post_type = 'attachment' AND post_status = 'trash' ",
                "SELECT post_name FROM wp_posts WHERE post_name = 'it\\'s' AND ID != 4 LIMIT 1",
                "SELECT post_name FROM wp_posts WHERE post_name = 'child' AND post_type IN ( 'page', 'attachment' ) \
                 AND ID != 4 AND post_parent = 2 LIMIT 1",
                "SELECT post_name FROM wp_posts WHERE post_name = 'hello' AND post_type = 'post' AND ID != 4 LIMIT 1",
            ]
        );

        // `esc_sql()` on the quoted lists: a quote is escaped, and a `%` (as `sanitize_title()` leaves in an
        // encoded slug) reaches the filter as `%`, not as `wpdb`'s placeholder escape.
        repo.pages_by_names(&slugs(&["it's", "%e2%9c%93"]), &types(&["page"]))
            .unwrap();
        assert_eq!(
            take(),
            [
                "\n\t\tSELECT ID, post_name, post_parent, post_type\n\t\tFROM wp_posts\n\t\tWHERE post_name IN \
              ('it\\'s','%e2%9c%93')\n\t\tAND post_type IN ('page')\n\t\tORDER BY\n\t\t\tpost_status = 'publish' \
              DESC,\n\t\t\tpost_status IN ('draft', 'pending', 'auto-draft') ASC, ID ASC\n\t"
            ]
        );

        // `wp_insert_post()`'s `$wpdb->insert( $wpdb->posts, $data )`, no formats: `$wpdb->field_types` makes the
        // ids and `menu_order` `%d`, the rest `%s`, in `compact()`'s order.
        repo.insert(
            &Post {
                author: 2,
                parent: PostParent::Int(3),
                menu_order: -1,
                ..named("new", "page")
            },
            Some(50),
        )
        .unwrap();
        assert_eq!(
            take(),
            [
                "INSERT INTO `wp_posts` (`post_author`, `post_date`, `post_date_gmt`, `post_content`, \
              `post_content_filtered`, `post_title`, `post_excerpt`, `post_status`, `post_type`, `comment_status`, \
              `ping_status`, `post_password`, `post_name`, `to_ping`, `pinged`, `post_modified`, \
              `post_modified_gmt`, `post_parent`, `menu_order`, `post_mime_type`, `guid`, `ID`) VALUES (2, \
              '0000-00-00 00:00:00', '0000-00-00 00:00:00', '', '', '', '', 'publish', 'page', 'open', 'open', '', \
              'new', '', '', '0000-00-00 00:00:00', '0000-00-00 00:00:00', 3, -1, '', '', 50)"
            ]
        );

        // A negative parent and import id go in as they are, and a `null` parent is written `NULL`.
        repo.insert(
            &Post {
                parent: PostParent::Int(-5),
                ..named("negative", "page")
            },
            Some(-5),
        )
        .unwrap();
        repo.insert(
            &Post {
                parent: PostParent::Null,
                ..named("orphan", "page")
            },
            None,
        )
        .unwrap();
        let taken = take();
        assert!(
            taken[0].ends_with("'0000-00-00 00:00:00', -5, 0, '', '', -5)"),
            "{taken:?}"
        );
        assert!(
            taken[1].ends_with("'0000-00-00 00:00:00', NULL, 0, '', '')"),
            "{taken:?}"
        );

        // `wp_insert_post()`'s `$wpdb->update( $wpdb->posts, $data, $where )`: the same columns, `ID` in the
        // condition.
        repo.update(&Post {
            id: PostId(50),
            author: 2,
            parent: PostParent::Null,
            ..named("new", "page")
        })
        .unwrap();
        assert_eq!(
            take(),
            [
                "UPDATE `wp_posts` SET `post_author` = 2, `post_date` = '0000-00-00 00:00:00', `post_date_gmt` = \
              '0000-00-00 00:00:00', `post_content` = '', `post_content_filtered` = '', `post_title` = '', \
              `post_excerpt` = '', `post_status` = 'publish', `post_type` = 'page', `comment_status` = 'open', \
              `ping_status` = 'open', `post_password` = '', `post_name` = 'new', `to_ping` = '', `pinged` = '', \
              `post_modified` = '0000-00-00 00:00:00', `post_modified_gmt` = '0000-00-00 00:00:00', \
              `post_parent` = NULL, `menu_order` = 0, `post_mime_type` = '', `guid` = '' WHERE `ID` = 50"
            ]
        );

        // The one-column updates, the delete and `wp_delete_post()`'s and `wp_delete_auto_drafts()`' reads.
        repo.update_name(PostId(50), &Slug::from("new__trashed"))
            .unwrap();
        repo.update_guid(PostId(50), b"http://example.org/?p=50")
            .unwrap();
        repo.update_status(PostId(50), &PostStatusName::new("publish"))
            .unwrap();
        repo.update_pinged(PostId(50), b"http://a.example/")
            .unwrap();
        repo.update_post_type(-3, &PostTypeName::new("page"))
            .unwrap();
        repo.import_id_taken(-5).unwrap();
        repo.raw_row(PostId(50)).unwrap();
        repo.children_of_type(PostId(50), &PostTypeName::new("page"))
            .unwrap();
        repo.reparent_children(PostId(50), PostParent::Int(3), &PostTypeName::new("page"))
            .unwrap();
        repo.revision_ids(PostId(50)).unwrap();
        repo.reparent_attachments(PostId(50), PostParent::Int(3))
            .unwrap();
        repo.remove(PostId(50)).unwrap();
        repo.auto_drafts_older_than_a_week().unwrap();
        assert_eq!(
            take(),
            [
                "UPDATE `wp_posts` SET `post_name` = 'new__trashed' WHERE `ID` = 50",
                "UPDATE `wp_posts` SET `guid` = 'http://example.org/?p=50' WHERE `ID` = 50",
                "UPDATE `wp_posts` SET `post_status` = 'publish' WHERE `ID` = 50",
                "UPDATE `wp_posts` SET `pinged` = 'http://a.example/' WHERE `ID` = 50",
                "UPDATE `wp_posts` SET `post_type` = 'page' WHERE `ID` = -3",
                "SELECT ID FROM wp_posts WHERE ID = -5",
                "SELECT * FROM wp_posts WHERE ID = 50",
                "SELECT * FROM wp_posts WHERE post_parent = 50 AND post_type = 'page'",
                "UPDATE `wp_posts` SET `post_parent` = 3 WHERE `post_parent` = 50 AND `post_type` = 'page'",
                "SELECT ID FROM wp_posts WHERE post_parent = 50 AND post_type = 'revision'",
                "UPDATE `wp_posts` SET `post_parent` = 3 WHERE `post_parent` = 50 AND `post_type` = 'attachment'",
                "DELETE FROM `wp_posts` WHERE `ID` = 50",
                "SELECT ID FROM wp_posts WHERE post_status = 'auto-draft' AND DATE_SUB( NOW(), INTERVAL 7 DAY ) > \
                 post_date",
            ]
        );
    });
}
