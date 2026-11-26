//! The native `wpdb` class and `$wpdb` from PHP, over a scripted fake engine (no MySQL needed).
//!
//! PHP boots once per process, so this test has its own `main()` (`harness = false`) and runs each case in order.

use std::any::Any;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_data::*;
use speedyspec_data_query::{EngineRegistry, mysql_text};
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, NativeArea, Php, PhpError};
use speedyspec_php_ext::{Value, twins_area};
use speedyspec_wp_db::WpDbConfig;
use speedyspec_wp_db_contracts::WpDbError;
use speedyspec_wp_db_php::{PhpWpdbState, WpDbArea};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::WpHooksArea;

fn check(name: &str, ok: bool, detail: impl std::fmt::Debug) {
    if ok {
        println!("ok: {name}");
    } else {
        eprintln!("FAIL: {name}: {detail:?}");
        std::process::exit(1);
    }
}

thread_local! {
    static SEEN: RefCell<Vec<(String, Vec<DbValue>)>> = const { RefCell::new(Vec::new()) };
}

struct NoDialect;
impl Dialect for NoDialect {
    fn kind(&self) -> DialectKind {
        DialectKind::MySql
    }
    fn prepare(&self, stmt: &Statement) -> Result<Prepared, DbError> {
        Ok(Prepared::Sql(vec![EmittedSql {
            sql: mysql_text(stmt.tree()),
            params: stmt.params().to_vec(),
        }]))
    }
}

struct NoIntrospector;
impl Introspector for NoIntrospector {
    fn server_version(&self) -> Result<String, DbError> {
        Ok("8.4.3".into())
    }
    fn tables(&self, _: Option<&str>) -> Result<Vec<String>, DbError> {
        Ok(vec![])
    }
    fn columns(&self, _: &str) -> Result<Vec<ColumnDef>, DbError> {
        Ok(vec![])
    }
    fn indexes(&self, _: &str) -> Result<Vec<IndexDef>, DbError> {
        Ok(vec![])
    }
    fn table_collation(&self, _: &str) -> Result<Option<String>, DbError> {
        Ok(None)
    }
}

fn rows(columns: &[&str], values: &[&[Option<&[u8]>]]) -> Rows {
    let names: Rc<[String]> = columns.iter().map(|c| c.to_string()).collect();
    Rows {
        columns: columns
            .iter()
            .map(|c| ColumnInfo {
                name: c.to_string(),
                ..ColumnInfo::default()
            })
            .collect(),
        rows: values
            .iter()
            .map(|r| {
                let values = r
                    .iter()
                    .map(|v| match v {
                        None => DbValue::Null,
                        Some(b) => match std::str::from_utf8(b) {
                            Ok(s) => DbValue::Text(s.into()),
                            Err(_) => DbValue::Bytes(b.to_vec()),
                        },
                    })
                    .collect();
                Row::new(names.clone(), values)
            })
            .collect(),
    }
}

/// Answers by the statement's text.
struct FakeStore;
impl FakeStore {
    fn answer(&self, stmt: &Statement) -> Result<Rows, DbError> {
        let text = mysql_text(stmt.tree());
        SEEN.with(|s| s.borrow_mut().push((text.clone(), stmt.params().to_vec())));
        if text.contains("missing") {
            return Err(DbError::Engine {
                engine: "default".into(),
                code: Some("1146".into()),
                message: "Table 'wp.missing' doesn't exist".into(),
            });
        }
        if text == "SELECT 1" {
            return Ok(rows(&["1"], &[&[Some(b"1")]]));
        }
        if text.starts_with("SELECT k, v FROM two") {
            return Ok(rows(
                &["k", "v"],
                &[&[Some(b"a"), Some(b"1")], &[Some(b"b"), Some(b"2")]],
            ));
        }
        if text.starts_with("SELECT bin") {
            return Ok(rows(&["bin"], &[&[Some(&[0xE9, 0x00, 0xFF])]]));
        }
        if text.starts_with("SHOW FULL COLUMNS") {
            return Ok(rows(
                &["Field", "Type", "Collation"],
                &[&[Some(b"a"), Some(b"varchar(20)"), Some(b"latin1_swedish_ci")]],
            ));
        }
        Ok(Rows::default())
    }
}
impl SqlStore for FakeStore {
    fn engine(&self) -> &str {
        "default"
    }
    fn dialect(&self) -> &dyn Dialect {
        &NoDialect
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            transactions: true,
            savepoints: true,
            ordered_ranges: true,
            expiry: false,
            atomic_increment: true,
            returning: false,
            upsert: UpsertForm::OnDuplicateKey,
        }
    }
    fn execute(&self, stmt: &Statement) -> Result<ExecResult, DbError> {
        self.answer(stmt).map(|_| ExecResult {
            rows_affected: 1,
            insert_id: Some(1),
        })
    }
    fn query(&self, stmt: &Statement) -> Result<Rows, DbError> {
        self.answer(stmt)
    }
    fn begin(&self) -> Result<(), DbError> {
        Ok(())
    }
    fn commit(&self) -> Result<(), DbError> {
        Ok(())
    }
    fn rollback(&self) -> Result<(), DbError> {
        Ok(())
    }
    fn savepoint(&self, _: &str) -> Result<(), DbError> {
        Ok(())
    }
    fn release_savepoint(&self, _: &str) -> Result<(), DbError> {
        Ok(())
    }
    fn rollback_to_savepoint(&self, _: &str) -> Result<(), DbError> {
        Ok(())
    }
    fn in_transaction(&self) -> bool {
        false
    }
    fn introspect(&self) -> &dyn Introspector {
        &NoIntrospector
    }
    fn reset(&self) -> Result<(), DbError> {
        Ok(())
    }
}

struct FakeEngine(String);
impl Engine for FakeEngine {
    fn name(&self) -> &str {
        &self.0
    }
    fn kind(&self) -> EngineKind {
        EngineKind::MySql
    }
    fn open(&self) -> Result<EngineHandle, DbError> {
        Ok(EngineHandle::Sql(Rc::new(FakeStore)))
    }
}

struct FakeFactory(&'static str);
impl EngineFactory for FakeFactory {
    fn scheme(&self) -> &'static str {
        self.0
    }
    fn create(&self, config: EngineConfig) -> Result<Rc<dyn Engine>, DbError> {
        Ok(Rc::new(FakeEngine(config.name)))
    }
}

fn eval(code: &str) -> Result<Value, PhpError> {
    speedyspec_php::eval("case", code)
}

fn yes(name: &str, code: &str) {
    let result = eval(code);
    check(name, matches!(result, Ok(Value::Bool(true))), result);
}

fn main() {
    let container = Rc::new(Container::new());
    let hooks = WpHooksArea::new(container.clone(), Rc::new(MemoryHookStore::new()));
    let registry = Rc::new(EngineRegistry::new());
    registry.register_factory(Rc::new(FakeFactory("fake")));
    registry.register_factory(Rc::new(FakeFactory("mysql")));
    registry
        .configure(EngineConfig {
            name: "default".into(),
            url: "fake://db".into(),
            options: BTreeMap::new(),
        })
        .unwrap();
    let config = WpDbConfig {
        user: "root".into(),
        password: "password".into(),
        name: "wp".into(),
        host: "localhost".into(),
        charset: Some("utf8mb4".into()),
        collate: None,
        table_prefix: "wp_".into(),
    };
    let wpdb = WpDbArea::new(container.clone(), hooks.api(), registry, config);
    let api = wpdb.api();
    let areas: Vec<Rc<dyn NativeArea>> = vec![
        twins_area(),
        Rc::new(ContainerArea::new("demo", container.clone())),
        Rc::new(hooks),
        Rc::new(wpdb),
    ];
    let _php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    })
    .expect("PHP boots");
    eval(
        r#"function _doing_it_wrong($f, $m, $v) { do_action('doing_it_wrong_run', $f, $m, $v); }
        $GLOBALS['wpdb']->set_prefix('wp_'); return true;"#,
    )
    .unwrap();

    yes(
        "wpdb_global_exists",
        "return $GLOBALS['wpdb'] instanceof wpdb && OBJECT === 'OBJECT' && ARRAY_N === 'ARRAY_N' && EZSQL_VERSION === 'WP1.25';",
    );
    yes(
        "query_returns_like_wordpress",
        r#"global $wpdb; $one = $wpdb->query('SELECT 1');
        $bad = $wpdb->query('SELECT * FROM missing');
        $err = $wpdb->last_error;
        $empty = $wpdb->query('');
        return $one === 1 && $bad === false && $err === "Table 'wp.missing' doesn't exist" && $empty === false;"#,
    );
    yes(
        "get_results_shapes",
        r#"global $wpdb; $o = $wpdb->get_results('SELECT k, v FROM two');
        $a = $wpdb->get_results('SELECT k, v FROM two', ARRAY_A);
        $n = $wpdb->get_results('SELECT k, v FROM two', ARRAY_N);
        $k = $wpdb->get_results('SELECT k, v FROM two', OBJECT_K);
        return $o[1] instanceof stdClass && $o[1]->v === '2'
            && $a[0] === ['k' => 'a', 'v' => '1'] && $n[1] === ['b', '2']
            && array_keys($k) === ['a', 'b'] && $k['b']->v === '2'
            && $wpdb->get_var('SELECT k, v FROM two', 1, 1) === '2'
            && $wpdb->get_row('SELECT k, v FROM two', ARRAY_A, 1) === ['k' => 'b', 'v' => '2']
            && $wpdb->get_col('SELECT k, v FROM two') === ['a', 'b']
            && $wpdb->num_rows === 2;"#,
    );
    yes(
        "get_results_same_objects",
        r#"global $wpdb; $r = $wpdb->get_results('SELECT k, v FROM two');
        return $r[0] === $wpdb->last_result[0] && $wpdb->get_row(null) === null && $wpdb->get_row('SELECT k, v FROM two') === $wpdb->last_result[0];"#,
    );
    yes(
        "prepare_from_php",
        r#"global $wpdb; $n = did_action('doing_it_wrong_run');
        $ok = $wpdb->prepare('%s', "a'b") === "'a\\'b'";
        $wpdb->prepare('SELECT 1');
        return $ok && did_action('doing_it_wrong_run') === $n + 1;"#,
    );
    yes(
        "php_query_filter_runs",
        r#"global $wpdb; add_filter('query', function ($q) { return $q === 'SELECT 2' ? 'SELECT 1' : $q; });
        $r = $wpdb->query('SELECT 2'); remove_all_filters('query');
        return $r === 1 && $wpdb->last_query === 'SELECT 1';"#,
    );
    yes(
        "subclass_works",
        r#"class mywpdb extends wpdb { function t() { return $this->get_table_from_query('SELECT * FROM a'); } }
        $db = new mywpdb('u', 'p', 'n', 'localhost');
        return $db->t() === 'a' && $db->query('SELECT 1') === 1 && $GLOBALS['wpdb']->num_rows !== null;"#,
    );
    yes(
        "helpers_from_php",
        r#"global $wpdb; return $wpdb->insert('wp_t', ['a' => 'x']) === 1 && $wpdb->insert_id === 1
            && $wpdb->update('wp_t', ['a' => 'y'], ['a' => 'x']) === 1 && $wpdb->delete('wp_t', ['a' => 'y']) === 1
            && $wpdb->update('wp_t', 'nope', []) === false
            && $wpdb->tables('blog', false)[0] === 'posts' && $wpdb->tables('blog')['posts'] === 'wp_posts'
            && $wpdb->esc_like('50%') === '50\\%' && $wpdb->quote_identifier('a`b') === '`a``b`';"#,
    );
    // ----- properties, magic methods and the state twin
    eval("function is_multisite() { return $GLOBALS['speedyspec_ms'] ?? false; } return true;")
        .unwrap();
    yes(
        "table_properties_follow_rust",
        r#"global $wpdb; $single = $wpdb->posts === 'wp_posts';
        $GLOBALS['speedyspec_ms'] = true; define('MULTISITE', true);
        $wpdb->set_prefix('wp_'); $wpdb->set_blog_id(2);
        $ms = $wpdb->posts === 'wp_2_posts' && $wpdb->users === 'wp_users' && $wpdb->prefix === 'wp_2_';
        $wpdb->set_blog_id(1); $GLOBALS['speedyspec_ms'] = false; $wpdb->set_prefix('wp_');
        $wpdb->posts = 'x';
        return $single && $ms;"#,
    );
    check(
        "table_property_written_from_php",
        api.table_name("posts") == "x",
        api.table_name("posts"),
    );
    eval("$GLOBALS['wpdb']->posts = 'wp_posts'; return true;").unwrap();
    yes(
        "protected_members_like_wordpress",
        r#"global $wpdb;
        $ok = !empty($wpdb->dbh); $dbh = $wpdb->dbh; $ok = $ok && !empty($dbh) && isset($wpdb->dbh);
        unset($wpdb->dbh); $ok = $ok && empty($wpdb->dbh);
        $wpdb->dbh = $dbh; $ok = $ok && !empty($wpdb->dbh);
        $new_meta = "HAHA I HOPE THIS DOESN'T WORK";
        $col_meta = $wpdb->col_meta; $wpdb->col_meta = $new_meta;
        $ok = $ok && $col_meta !== $new_meta && $col_meta === $wpdb->col_meta;
        $ok = $ok && empty($wpdb->nonexistent_property);
        $wpdb->nonexistent_property = true;
        $ok = $ok && $wpdb->nonexistent_property === true && isset($wpdb->nonexistent_property);
        unset($wpdb->nonexistent_property);
        return $ok && empty($wpdb->nonexistent_property);"#,
    );
    yes(
        "custom_table_property",
        r#"global $wpdb; $n = did_action('deprecated_function_run'); $wpdb->mytable = 'wp_mytable';
        return $wpdb->mytable === 'wp_mytable' && did_action('deprecated_function_run') === $n;"#,
    );
    SEEN.with(|s| s.borrow_mut().clear());
    yes(
        "binary_bytes_round_trip",
        r#"global $wpdb; $v = $wpdb->get_var('SELECT bin FROM b');
        $q = $wpdb->prepare('INSERT INTO latin VALUES (%s)', "\xE9");
        return $v === "\xE9\x00\xFF" && $q === "INSERT INTO latin VALUES ('\xE9')" && $wpdb->query($q) === 1;"#,
    );
    let sent = SEEN.with(|s| {
        s.borrow()
            .iter()
            .find(|(t, _)| t.starts_with("INSERT INTO latin"))
            .cloned()
    });
    check(
        "binary_reaches_store",
        matches!(&sent, Some((_, p)) if p == &vec![DbValue::Bytes(vec![0xE9])]),
        &sent,
    );
    let twin = eval(
        r#"final class ArrayWpdbState implements \SpeedySpec\WpDb\WpdbState
        {
            public static array $stored = [];
            private array $state = [];
            public function load(): array { return $this->state; }
            public function store(array $state): void { $this->state = $state; self::$stored[] = $state; }
            public function reset(): void { $this->state = []; }
        }
        return new ArrayWpdbState();"#,
    )
    .unwrap();
    let twin: Rc<dyn speedyspec_wp_db::WpdbState> = Rc::new(PhpWpdbState::new(
        speedyspec_php_ext::PhpObject::new(twin).expect("a PHP object"),
    ));
    let memory = container
        .make_port::<dyn speedyspec_wp_db::WpdbState>(speedyspec_wp_db::WPDB_STATE)
        .unwrap();
    let bound = twin.clone();
    container.singleton_port::<dyn speedyspec_wp_db::WpdbState>(
        speedyspec_wp_db::WPDB_STATE,
        move |_, _| Ok(bound.clone()),
    );
    let stored = eval(
        r#"global $wpdb; $wpdb->query('SELECT 1'); $last = end(ArrayWpdbState::$stored);
        return $last['last_query'] === 'SELECT 1' && $last['num_rows'] === 1 && $wpdb->last_query === 'SELECT 1';"#,
    );
    container.singleton_port::<dyn speedyspec_wp_db::WpdbState>(
        speedyspec_wp_db::WPDB_STATE,
        move |_, _| Ok(memory.clone()),
    );
    check(
        "php_state_twin_replaces_memory",
        matches!(stored, Ok(Value::Bool(true))),
        stored,
    );
    yes(
        "dbh_not_mysqli",
        r#"global $wpdb; return !($wpdb->dbh instanceof mysqli) && !empty($wpdb->dbh) && $wpdb->dbh instanceof \SpeedySpec\WpDb\Connection
            && $wpdb->is_mysql === true;"#,
    );
    yes(
        "hook_exception_reaches_php",
        r#"global $wpdb; add_filter('query', function ($q) { throw new RuntimeException('boom'); });
        try { $wpdb->query('SELECT 1'); $r = false; } catch (RuntimeException $e) { $r = $e->getMessage() === 'boom'; }
        remove_all_filters('query'); return $r;"#,
    );
    yes(
        "col_info_and_last_result_writes",
        r#"global $wpdb; $wpdb->query('SELECT k, v FROM two'); $info = $wpdb->col_info;
        $wpdb->last_result = [(object) ['a' => 'x']];
        return is_array($info) && $info[0]->name === 'k' && $wpdb->get_var(null) === 'x' && $wpdb->last_result[0]->a === 'x';"#,
    );
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("db.php"), "<?php").unwrap();
    let refused = WpDbArea::check_drop_in(tmp.path());
    let path = tmp.path().join("db.php").display().to_string();
    check(
        "drop_in_refused",
        matches!(&refused, Err(e @ WpDbError::DropInRefused { .. }) if e.to_string().contains(&path)),
        &refused,
    );
    check(
        "drop_in_absent",
        WpDbArea::check_drop_in(&tmp.path().join("none")).is_ok(),
        (),
    );
    let _ = Rc::new(()) as Rc<dyn Any>;
}
