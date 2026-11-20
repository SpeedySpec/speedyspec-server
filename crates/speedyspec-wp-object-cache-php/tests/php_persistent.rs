//! The native object cache over a second level, from PHP. Its own process: the area last made owns the natives.

use std::rc::Rc;

use speedyspec_cache::CachingStore;
use speedyspec_cache_memory::MemoryCache;
use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, NativeArea, Php};
use speedyspec_php_ext::{Value, twins_area};
use speedyspec_wp_object_cache_php::ObjectCacheArea;

fn check(name: &str, ok: bool, detail: impl std::fmt::Debug) {
    if ok {
        println!("ok: {name}");
    } else {
        eprintln!("FAIL: {name}: {detail:?}");
        std::process::exit(1);
    }
}

fn main() {
    let container = Rc::new(Container::new());
    let store = Rc::new(MemoryCache::new());
    let area = ObjectCacheArea::new(
        container.clone(),
        Some(store.clone() as Rc<dyn CachingStore>),
    );
    let areas: Vec<Rc<dyn NativeArea>> = vec![
        twins_area(),
        Rc::new(ContainerArea::new("demo", container)),
        Rc::new(area),
    ];
    let _php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    })
    .expect("PHP boots");

    let r = speedyspec_php::eval(
        "case",
        "wp_cache_init(); $f = function () { return 7; }; wp_cache_set('c', $f, 'g'); wp_cache_set('s', 'plain', 'g');
        $back = wp_cache_get('c', 'g'); return $back instanceof Closure && $back() === 7;",
    );
    check(
        "closure_readable_in_request",
        matches!(r, Ok(Value::Bool(true))),
        &r,
    );
    let closure = store.get("g", "c");
    let plain = store.get("g", "s");
    check(
        "closure_refused_by_second_level",
        matches!(closure, Ok(None)) && matches!(&plain, Ok(Some(b)) if b == b"s:5:\"plain\";"),
        (&closure, &plain),
    );
}
