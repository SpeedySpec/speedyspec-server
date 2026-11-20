//! The native wp_cache_* functions and WP_Object_Cache from PHP.
//!
//! PHP boots once per process, so this test has its own `main()` (`harness = false`) and runs each case in order.

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_cqrs::QueryBus;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, NativeArea, Php, PhpError};
use speedyspec_php_ext::{Value, boundary_crossings, twins_area};
use speedyspec_php_serialize::{from_hook_value, serialize, unserialize};
use speedyspec_wp_object_cache::CacheGet;
use speedyspec_wp_object_cache_php::ObjectCacheArea;

fn check(name: &str, ok: bool, detail: impl std::fmt::Debug) {
    if ok {
        println!("ok: {name}");
    } else {
        eprintln!("FAIL: {name}: {detail:?}");
        std::process::exit(1);
    }
}

fn eval(code: &str) -> Result<Value, PhpError> {
    speedyspec_php::eval("case", code)
}

fn yes(name: &str, code: &str) {
    let r = eval(code);
    check(name, matches!(r, Ok(Value::Bool(true))), r);
}

fn php_bytes(v: Value) -> Vec<u8> {
    match v {
        Value::Str(s) => s.into_bytes(),
        Value::Bytes(b) => b,
        other => panic!("{other:?}"),
    }
}

fn main() {
    let container = Rc::new(Container::new());
    let area = ObjectCacheArea::new(container.clone(), None);
    let areas: Vec<Rc<dyn NativeArea>> = vec![
        twins_area(),
        Rc::new(ContainerArea::new("demo", container.clone())),
        Rc::new(area),
    ];
    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    })
    .expect("PHP boots");
    eval("wp_cache_init(); return true;").unwrap();

    yes(
        "functions_declared",
        r#"foreach (['wp_cache_init','wp_cache_add','wp_cache_add_multiple','wp_cache_replace','wp_cache_set','wp_cache_set_multiple',
            'wp_cache_get','wp_cache_get_multiple','wp_cache_delete','wp_cache_delete_multiple','wp_cache_incr','wp_cache_decr',
            'wp_cache_flush','wp_cache_flush_runtime','wp_cache_flush_group','wp_cache_supports','wp_cache_close',
            'wp_cache_add_global_groups','wp_cache_add_non_persistent_groups','wp_cache_switch_to_blog','wp_cache_reset'] as $f) {
            if (!function_exists($f)) { return $f; }
        }
        return $GLOBALS['wp_object_cache'] instanceof WP_Object_Cache;"#,
    );
    yes(
        "object_refs",
        r#"$key = 'objectrefs'; $object_a = new stdClass(); $object_a->foo = 'alpha';
        wp_cache_set($key, $object_a); $object_a->foo = 'bravo';
        $object_b = wp_cache_get($key); $object_b->foo = 'charlie';
        return wp_cache_get($key)->foo === 'alpha' && $object_b->foo === 'charlie';"#,
    );
    yes(
        "found_by_ref",
        r#"wp_cache_set('f', false); $v = wp_cache_get('f', '', false, $found); $miss = wp_cache_get('nope', '', false, $found2);
        return $v === false && $found === true && $miss === false && $found2 === false;"#,
    );
    yes(
        "instances_are_separate",
        r#"wp_cache_set('k', 'global'); $a = new WP_Object_Cache(); $b = new WP_Object_Cache();
        return $a->get('k') === false && $a == $b && wp_cache_get('k') === 'global';"#,
    );
    yes(
        "subclass_method_called",
        r#"class MyCache extends WP_Object_Cache { public function get($key, $group = 'default', $force = false, &$found = null) { $found = true; return 'override'; } }
        $saved = $GLOBALS['wp_object_cache']; $GLOBALS['wp_object_cache'] = new MyCache();
        $r = wp_cache_get('anything'); $GLOBALS['wp_object_cache'] = $saved; return $r === 'override';"#,
    );
    eval("wp_cache_set('rk', 'v', 'g'); return true;").unwrap();
    let got = QueryBus::new(container.clone())
        .ask(CacheGet {
            key: Value::Str("rk".into()),
            group: Value::Str("g".into()),
            force: false,
        })
        .unwrap();
    check(
        "rust_caller_sees_php_set",
        got == Ok(Some(Value::Str("v".into()))),
        &got,
    );
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("object-cache.php"), "<?php").unwrap();
    let dir = tmp.path().display().to_string();
    let refused = eval(&format!(
        "define('WP_CONTENT_DIR', '{dir}'); try {{ wp_cache_init(); return false; }} catch (Error $e) {{ return $e->getMessage(); }}"
    ));
    check(
        "drop_in_refused",
        matches!(&refused, Ok(Value::Str(m)) if m.contains(&dir) && m.contains("not supported")),
        &refused,
    );
    std::fs::remove_file(tmp.path().join("object-cache.php")).unwrap();
    eval("wp_cache_init(); wp_cache_set('hot', 1); return true;").unwrap();
    // The eval that runs the loop is itself one entry: compare with the same loop without the calls.
    let before = boundary_crossings();
    eval("for ($i = 0; $i < 100; $i++) { } return true;").unwrap();
    let baseline = boundary_crossings() - before;
    let before = boundary_crossings();
    eval("for ($i = 0; $i < 100; $i++) { wp_cache_get('hot'); } return true;").unwrap();
    let used = boundary_crossings() - before;
    check(
        "no_boundary_crossing_for_get",
        used == baseline,
        (used, baseline),
    );
    eval("$o = new stdClass(); $o->a = [1, new stdClass()]; wp_cache_set('obj', $o); wp_cache_set('arr', [$o, 'x']); return true;").unwrap();
    php.restart_request().unwrap();
    eval("wp_cache_init(); return true;").unwrap();
    yes(
        "values_released_at_request_end",
        "return wp_cache_get('obj') === false && wp_cache_get('arr') === false;",
    );

    // serialize_matches_php: Rust's serialize equals PHP's for values Rust can hold, and Rust re-serializes
    // PHP's bytes of any value to the same bytes.
    let mut floats = vec![
        0.1,
        1.0,
        -0.0,
        1e14,
        1e15,
        0.0001,
        0.00001,
        1.5e300,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
        1.0 / 3.0,
    ];
    let mut seed: u64 = 0x5eed;
    for _ in 0..1000 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        floats.push(f64::from_bits(seed));
    }
    let mut values: Vec<Value> = floats.into_iter().map(Value::Float).collect();
    values.extend([
        Value::Int(0),
        Value::Int(-7),
        Value::Int(i64::MAX),
        Value::Int(i64::MIN),
        Value::Null,
        Value::Bool(true),
    ]);
    values.extend([
        Value::Str("a\0\"b'c".into()),
        Value::Str("é漢🎷".into()),
        Value::Bytes(vec![0xff, 0xfe, b'x']),
    ]);
    values.push(Value::Array(vec![
        (speedyspec_php_ext::Key::Str("5x".into()), Value::Int(1)),
        (
            speedyspec_php_ext::Key::Int(5),
            Value::Array(vec![(
                speedyspec_php_ext::Key::Int(0),
                Value::Str("n".into()),
            )]),
        ),
    ]));
    let mut mismatches = Vec::new();
    for v in &values {
        let theirs = php_bytes(
            speedyspec_php::call(&Value::Str("serialize".into()), std::slice::from_ref(v)).unwrap(),
        );
        let ours = serialize(&from_hook_value(v).unwrap());
        if theirs != ours {
            mismatches.push((
                String::from_utf8_lossy(&theirs).into_owned(),
                String::from_utf8_lossy(&ours).into_owned(),
            ));
        }
    }
    let php_side = eval(
        r#"class Priv { private $a = 1; protected $b = [2]; public $c = 'c'; }
        class Ser implements Serializable { public function serialize() { return 'payload'; } public function unserialize($d) {} public function __serialize(): array { return ['p' => 1]; } public function __unserialize(array $d): void {} }
        enum Suit: string { case Hearts = 'H'; }
        $x = [1, 2]; $pair = [&$x, &$x]; $o = new stdClass(); $o->self = 'v';
        return array_map('serialize', [$pair, $o, [$o, $o], new Priv(), new ArrayObject([1, 'a' => 2]), new Ser(), Suit::Hearts, [Suit::Hearts]]);"#,
    )
    .unwrap();
    if let Value::Array(items) = php_side {
        for (_, v) in items {
            let bytes = php_bytes(v);
            match unserialize(&bytes) {
                Ok(parsed) if serialize(&parsed) == bytes => {}
                other => mismatches.push((
                    String::from_utf8_lossy(&bytes).into_owned(),
                    format!("{other:?}"),
                )),
            }
        }
    }
    check(
        "serialize_matches_php",
        mismatches.is_empty(),
        &mismatches[..mismatches.len().min(5)],
    );
}
