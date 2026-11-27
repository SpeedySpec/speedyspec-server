//! The native post type and post status registries from PHP over MySQL: the recorded `types-*` and `statuses-*`
//! scenarios of `compat/posts-read` (single site and multisite) replayed exactly, a plugin's mutation of
//! `$wp_post_types` seen live, `create_initial_post_types()` against the release's own PHP, and exceptions reaching
//! the caller unchanged with nothing registered. PHP boots once per process (`harness = false`).
//!
//! Needs MySQL (`SPEEDYSPEC_TEST_MYSQL_URL`); without it the test reports that it was skipped. It runs on a database
//! of its own (`<database>_posts_read`), because compat/posts-read/setup.php empties every table it seeds.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::Path;
use std::rc::Rc;

use speedyspec_compat::{Catalogue, replay_with, wordpress_release};
use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_data::EngineConfig;
use speedyspec_data_mysql::MySqlEngineFactory;
use speedyspec_data_query::{EngineRegistry, parse_one};
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, NativeArea, Php};
use speedyspec_php_ext::{Value, twins_area};
use speedyspec_wp_db::WpDbConfig;
use speedyspec_wp_db_php::WpDbArea;
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::WpHooksArea;
use speedyspec_wp_object_cache_php::ObjectCacheArea;
use speedyspec_wp_posts_php::{PostsArea, prepare_release};

/// `url` with its database replaced by `<database>_<suffix>`, which is made when missing.
fn own_database(url: &str, suffix: &str) -> String {
    let (base, name) = url.rsplit_once('/').expect("the URL names a database");
    let registry = EngineRegistry::new();
    registry.register_factory(Rc::new(MySqlEngineFactory));
    registry
        .configure(EngineConfig {
            name: "default".into(),
            url: url.to_owned(),
            options: BTreeMap::new(),
        })
        .unwrap();
    let statement = parse_one(&format!("CREATE DATABASE IF NOT EXISTS {name}_{suffix}")).unwrap();
    registry
        .sql("default")
        .and_then(|sql| sql.execute(&statement))
        .unwrap();
    format!("{base}/{name}_{suffix}")
}

/// Boots PHP with the Hook API, `wpdb`, the object cache and the post type registries, over `url`.
fn boot(url: &str) -> Php {
    let container = Rc::new(Container::new());
    let hooks = WpHooksArea::new(container.clone(), Rc::new(MemoryHookStore::new()));
    let registry = Rc::new(EngineRegistry::new());
    registry.register_factory(Rc::new(MySqlEngineFactory));
    registry
        .configure(EngineConfig {
            name: "default".into(),
            url: url.to_owned(),
            options: BTreeMap::new(),
        })
        .unwrap();
    let rest = url.trim_start_matches("mysql://");
    let (auth, host_db) = rest.split_once('@').expect("user@host");
    let (user, password) = auth.split_once(':').unwrap_or((auth, ""));
    let (host, name) = host_db.split_once('/').expect("host/database");
    let config = WpDbConfig {
        user: user.into(),
        password: password.into(),
        name: name.into(),
        host: host.into(),
        charset: Some("utf8mb4".into()),
        collate: Some(String::new()),
        table_prefix: "wp_".into(),
    };
    let wpdb = WpDbArea::new(container.clone(), hooks.api(), registry, config);
    let cache = ObjectCacheArea::new(container.clone(), None);
    let posts = PostsArea::new(hooks.api());
    let areas: Vec<Rc<dyn NativeArea>> = vec![
        twins_area(),
        Rc::new(ContainerArea::new("tests", container)),
        Rc::new(hooks),
        Rc::new(wpdb),
        Rc::new(cache),
        Rc::new(posts),
    ];
    Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    })
    .expect("PHP boots")
}

/// The code that loads compat/posts-read/setup.php over the prepared release.
fn setup_code(root: &Path, multisite: bool) -> String {
    format!(
        "define('SPEEDYSPEC_COMPAT_WP_ROOT', {:?}); define('SPEEDYSPEC_COMPAT_PORT', true); \
         define('SPEEDYSPEC_COMPAT_MULTISITE', {multisite}); require {:?};",
        format!("{}/", root.display()),
        speedyspec_compat::compat_dir()
            .join("posts-read/setup.php")
            .display()
            .to_string()
    )
}

fn check(name: &str, ok: bool, detail: impl std::fmt::Debug) -> bool {
    if ok {
        println!("ok: {name}");
    } else {
        eprintln!("FAIL: {name}: {detail:?}");
    }
    ok
}

/// Runs `code` in a fresh request with the single-site setup loaded; passes when it returns `true`.
fn yes(php: &Php, setup: &str, name: &str, code: &str) -> bool {
    php.restart_request().unwrap();
    if let Err(e) = php.eval("speedyspec://posts-registry/setup", setup) {
        return check(name, false, e);
    }
    let r = php.eval("speedyspec://posts-registry/case", code);
    check(name, matches!(r, Ok(Value::Bool(true))), r)
}

/// The `types-*` and `statuses-*` scenarios replayed through the native functions; each scenario's request is a
/// multisite install while `multisite` holds true.
fn replay(runner: &mut speedyspec_compat::PhpRunner, multisite: &Cell<bool>) -> bool {
    let only = std::env::var("SPEEDYSPEC_COMPAT_ONLY").ok();
    let mut ok = true;
    for network in [false, true] {
        multisite.set(network);
        let (catalogue, fixture, breaks) =
            speedyspec_compat::load_for("posts-read", network).expect("the posts-read files load");
        let scenarios: Vec<_> = catalogue
            .scenarios
            .iter()
            .filter(|s| s.name.starts_with("types-") || s.name.starts_with("statuses-"))
            .filter(|s| only.as_ref().is_none_or(|o| *o == s.name))
            .cloned()
            .collect();
        let total = scenarios.len();
        let filtered = Catalogue {
            area: catalogue.area.clone(),
            scenarios,
        };
        let mismatches = replay_with(&filtered, &fixture, &breaks, runner);
        println!(
            "{}: {} of {total} types-/statuses- scenarios match",
            if network { "multisite" } else { "single site" },
            total - mismatches.len()
        );
        for m in &mismatches {
            eprintln!("MISMATCH {m}");
            let json = |v: &Option<speedyspec_compat::StepResult>| {
                v.as_ref()
                    .map(|e| serde_json::to_string(e).unwrap_or_default())
                    .unwrap_or_default()
            };
            eprintln!("  expected: {}", json(&m.expected));
            eprintln!("  actual:   {}", json(&m.actual));
        }
        ok &= check(
            "registry_replay_matches",
            mismatches.is_empty(),
            mismatches.len(),
        );
    }
    ok
}

/// The text of each named `function` in `source`, through the `}` that ends it.
fn functions(source: &str, names: &[&str]) -> String {
    let lines: Vec<&str> = source.split_inclusive('\n').collect();
    let mut out = String::new();
    for name in names {
        let start = lines
            .iter()
            .position(|l| l.starts_with(&format!("function {name}(")))
            .unwrap_or_else(|| panic!("post.php has no {name}()"));
        let end = (start..lines.len())
            .find(|&i| lines[i].trim_end() == "}")
            .unwrap();
        out.extend(lines[start..=end].iter().copied());
    }
    out
}

/// WordPress's own registry functions and `WP_Post_Type` from the release, for a `SpeedySpec\Release` namespace
/// block, so they run beside the native ones: inside the namespace their calls to each other resolve to these copies.
fn release_registry(release: &Path) -> String {
    let post = std::fs::read_to_string(release.join("wp-includes/post.php")).unwrap();
    let class =
        std::fs::read_to_string(release.join("wp-includes/class-wp-post-type.php")).unwrap();
    let class = class.trim_start_matches("<?php");
    let functions = functions(
        &post,
        &[
            "create_initial_post_types",
            "register_post_status",
            "register_post_type",
            "get_post_type_capabilities",
            "_post_type_meta_capabilities",
            "get_post_type_labels",
            "_get_custom_object_labels",
            "add_post_type_support",
            "remove_post_type_support",
            "post_type_supports",
            "is_post_type_viewable",
            "get_post_type_object",
            "post_type_exists",
        ],
    );
    format!("{class}\n{functions}").replace("new WP_Error", "new \\WP_Error")
}

/// The registries as plain data: objects as their short class name and properties.
const SNAPSHOT: &str = r#"
function speedyspec_registry_plain($value)
{
    if (is_array($value)) {
        return array_map('speedyspec_registry_plain', $value);
    }
    if (is_object($value)) {
        $class = get_class($value);
        $short = substr($class, (int) strrpos('\\' . $class, '\\'));
        return ['class' => $short, 'props' => speedyspec_registry_plain(get_object_vars($value))];
    }
    return $value;
}
function speedyspec_registry_snapshot()
{
    global $wp_post_types, $wp_post_statuses, $_wp_post_type_features, $post_type_meta_caps;
    return speedyspec_registry_plain([
        'types' => $wp_post_types,
        'statuses' => $wp_post_statuses,
        'features' => $_wp_post_type_features,
        'meta_caps' => $post_type_meta_caps,
    ]);
}
function speedyspec_registry_reset()
{
    global $wp_post_types, $wp_post_statuses, $_wp_post_type_features, $post_type_meta_caps;
    $wp_post_types = $wp_post_statuses = $_wp_post_type_features = $post_type_meta_caps = null;
}
function speedyspec_registry_diff($a, $b, $path = '')
{
    if (is_array($a) && is_array($b)) {
        if (array_keys($a) !== array_keys($b)) {
            return [$path, 'keys', array_keys($a), array_keys($b)];
        }
        foreach ($a as $k => $v) {
            $d = speedyspec_registry_diff($v, $b[$k], "$path/$k");
            if ($d !== null) {
                return $d;
            }
        }
        return null;
    }
    return $a === $b ? null : [$path, $a, $b];
}
"#;

fn main() {
    let Ok(url) = std::env::var("SPEEDYSPEC_TEST_MYSQL_URL") else {
        println!("skipped: needs MySQL: set SPEEDYSPEC_TEST_MYSQL_URL");
        return;
    };
    let url = own_database(&url, "posts_read");
    // compat/posts-read/setup.php connects through this variable.
    // SAFETY: no other thread runs yet.
    unsafe { std::env::set_var("SPEEDYSPEC_TEST_MYSQL_URL", &url) };
    let release = wordpress_release(&speedyspec_compat::area_wordpress("posts-read"))
        .expect("the pinned release");
    let root = prepare_release(
        &release,
        &release.with_file_name("wordpress-posts-registry-port"),
    )
    .expect("the release prepares");
    let multisite = Rc::new(Cell::new(false));
    let codes = (setup_code(&root, false), setup_code(&root, true));
    let flag = multisite.clone();
    let mut runner = speedyspec_compat::PhpRunner::new(boot(&url), move |php| {
        let code = if flag.get() { &codes.1 } else { &codes.0 };
        php.eval("speedyspec://compat/posts-read-setup", code)
            .map(|_| ())
            .map_err(|e| e.to_string())
    });
    let mut ok = replay(&mut runner, &multisite);
    let php = runner.php();
    let setup = setup_code(&root, false);

    // Review Focus 4: a plugin mutates the object after init, then replaces the entry; each read sees it.
    ok &= yes(
        php,
        &setup,
        "mutated_global_is_seen",
        r#"global $wp_post_types;
        $wp_post_types['post']->hierarchical = true;
        $wp_post_types['post']->label = 'Articles';
        $mutated = get_post_type_object('post');
        $seen = $mutated === $wp_post_types['post'] && true === is_post_type_hierarchical('post')
            && in_array('post', get_post_types(array('hierarchical' => true)), true)
            && array('post' => 'post') === get_post_types(array('label' => 'Articles'));
        $replacement = clone $wp_post_types['post'];
        $replacement->publicly_queryable = false;
        $replacement->public = false;
        $wp_post_types['post'] = $replacement;
        unset($wp_post_types['page']);
        return $seen && get_post_type_object('post') === $replacement && !is_post_type_viewable('post')
            && !post_type_exists('page') && null === get_post_type_object('page');"#,
    );

    let compare = format!(
        "namespace {{\n{SNAPSHOT}\n}}\nnamespace SpeedySpec\\Release {{\n{}\n}}\nnamespace {{\n{}\n}}",
        release_registry(&release),
        r#"speedyspec_registry_reset();
        \create_initial_post_types();
        $native = speedyspec_registry_snapshot();
        speedyspec_registry_reset();
        \SpeedySpec\Release\create_initial_post_types();
        $release = speedyspec_registry_snapshot();
        $diff = speedyspec_registry_diff($native, $release);
        return $diff === null ? true : json_encode($diff);"#
    );
    ok &= yes(
        php,
        &setup,
        "create_initial_post_types_matches_release",
        &compare,
    );

    // The label and capability helpers a plugin (or WP_Post_Type::set_props(), or taxonomy.php) calls directly,
    // with their side effects on the object and on `$post_type_meta_caps`.
    let helpers = format!(
        "namespace {{\n{SNAPSHOT}\n}}\nnamespace SpeedySpec\\Release {{\n{}\n}}\nnamespace {{\n{}\n}}",
        release_registry(&release),
        r#"global $post_type_meta_caps;
        $run = static function (string $ns) {
            global $post_type_meta_caps;
            $post_type_meta_caps = array();
            $object = (object) array(
                'name' => 'tome', 'label' => 'Tomes', 'labels' => array('singular_name' => 'Tome'),
                'hierarchical' => true, 'capability_type' => 'tome', 'map_meta_cap' => true,
                'capabilities' => array('read' => 'read_tomes'),
            );
            $out = array(
                ($ns . 'get_post_type_capabilities')($object),
                ($ns . 'get_post_type_labels')($object),
                ($ns . '_get_custom_object_labels')($object, array('name' => array('X', 'Y'), 'parent' => array(null, 'P:'))),
                $object,
                $post_type_meta_caps,
            );
            return speedyspec_registry_plain($out);
        };
        $diff = speedyspec_registry_diff($run('\\'), $run('\\SpeedySpec\\Release\\'));
        return $diff === null ? true : json_encode($diff);"#
    );
    ok &= yes(
        php,
        &setup,
        "label_and_capability_helpers_match_release",
        &helpers,
    );

    // A callback's exception during register_post_type() reaches the caller as the same object, and nothing of the
    // post type is registered.
    ok &= yes(
        php,
        &setup,
        "register_post_type_args_exception_is_rethrown",
        r#"global $_wp_post_type_features, $post_type_meta_caps;
        $caps = $post_type_meta_caps;
        $thrown = new RuntimeException('from register_post_type_args');
        $cb = function () use ($thrown) { throw $thrown; };
        add_filter('register_post_type_args', $cb);
        try { register_post_type('thrower', array('map_meta_cap' => true, 'capability_type' => 'tome')); $caught = null; }
        catch (RuntimeException $e) { $caught = $e; }
        remove_filter('register_post_type_args', $cb);
        return $caught === $thrown && !post_type_exists('thrower') && !isset($_wp_post_type_features['thrower'])
            && $caps === $post_type_meta_caps;"#,
    );
    ok &= yes(
        php,
        &setup,
        "labels_filter_exception_is_rethrown",
        r#"global $_wp_post_type_features;
        $thrown = new DomainException('from post_type_labels_tome');
        $cb = function () use ($thrown) { throw $thrown; };
        add_filter('post_type_labels_tome', $cb);
        try { register_post_type('tome', array('rewrite' => false, 'query_var' => false)); $caught = null; }
        catch (DomainException $e) { $caught = $e; }
        remove_filter('post_type_labels_tome', $cb);
        return $caught === $thrown && !post_type_exists('tome') && !isset($_wp_post_type_features['tome']);"#,
    );
    ok &= yes(
        php,
        &setup,
        "sanitize_key_exception_is_rethrown",
        r#"$thrown = new LengthException('from sanitize_key');
        $cb = function () use ($thrown) { throw $thrown; };
        add_filter('sanitize_key', $cb);
        try { register_post_status('held'); $caught = null; } catch (LengthException $e) { $caught = $e; }
        remove_filter('sanitize_key', $cb);
        return $caught === $thrown && null === get_post_status_object('held');"#,
    );

    if !ok {
        std::process::exit(1);
    }
}
