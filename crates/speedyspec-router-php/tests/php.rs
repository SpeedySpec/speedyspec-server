//! The route table store supplied from PHP. PHP boots once per process (`harness = false`).

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_http::Method;
use speedyspec_php::{BootOptions, NativeArea, Php};
use speedyspec_php_ext::twins_area;
use speedyspec_router_php::{BoundRouteTableStore, register_route_table_port};
use speedyspec_router_store_memory::MemoryRouteTableStore;
use speedyspec_routing::{
    Fingerprint, RouteData, RouteId, RouteTableStore, StaticRoute, conformance,
};

fn check(name: &str, ok: bool, detail: impl std::fmt::Debug) {
    if ok {
        println!("ok: {name}");
    } else {
        eprintln!("FAIL: {name}: {detail:?}");
        std::process::exit(1);
    }
}

const STORES: &str = r#"
class ArrayRouteTableStore implements \SpeedySpec\Routing\RouteTableStore {
    public array $entries = [];
    public function get(string $fingerprint): mixed { return $this->entries[$fingerprint] ?? null; }
    public function put(string $fingerprint, mixed $data): void { $this->entries[$fingerprint] = $data; }
    public function forget(string $fingerprint): void { unset($this->entries[$fingerprint]); }
}
class GarbageRouteTableStore extends ArrayRouteTableStore {
    public function get(string $fingerprint): mixed { return 'nope'; }
}
class ThrowingRouteTableStore extends ArrayRouteTableStore {
    public function get(string $fingerprint): mixed { throw new RuntimeException('store down'); }
}
function bind_store(string $class): void {
    \SpeedySpec\Container\Container::getInstance()->instance('SpeedySpec\Routing\RouteTableStore', new $class());
}
return true;
"#;

fn data() -> RouteData {
    RouteData {
        statics: vec![StaticRoute {
            method: Method::Get,
            path: "/a".into(),
            id: RouteId(0),
        }],
        ..RouteData::default()
    }
}

fn main() {
    let container = Rc::new(Container::new());
    let areas: Vec<Rc<dyn NativeArea>> = vec![
        twins_area(),
        Rc::new(register_route_table_port(ContainerArea::new(
            "demo",
            container.clone(),
        ))),
    ];
    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    })
    .expect("PHP boots");
    let memory = Rc::new(MemoryRouteTableStore::new());
    let store = BoundRouteTableStore::new(container.clone(), memory.clone());
    let fingerprint = Fingerprint([7; 32]);

    let declared = php.eval("stores", STORES);
    check("php_stores_declare", declared.is_ok(), &declared);

    let bound = php.eval("bind", "bind_store('ArrayRouteTableStore'); return true;");
    check("php_store_binds", bound.is_ok(), &bound);
    let result = conformance::check(&store);
    check("php_store_conforms", result.is_ok(), &result);
    store.put(&fingerprint, &data()).unwrap();
    check(
        "php_store_holds_the_entry",
        memory.get(&fingerprint).unwrap().is_none(),
        "memory was written",
    );

    php.eval("bind", "bind_store('GarbageRouteTableStore'); return true;")
        .unwrap();
    let got = store.get(&fingerprint);
    check("garbage_from_php_store_is_a_miss", got == Ok(None), &got);

    php.eval(
        "bind",
        "bind_store('ThrowingRouteTableStore'); return true;",
    )
    .unwrap();
    let got = store.get(&fingerprint);
    check(
        "php_exception_is_store_error",
        matches!(&got, Err(e) if e.0.contains("store down")),
        &got,
    );

    php.restart_request().unwrap();
    store.put(&fingerprint, &data()).unwrap();
    check(
        "unbound_uses_memory",
        memory.get(&fingerprint).unwrap() == Some(data()),
        memory.get(&fingerprint),
    );

    drop(php);
    memory.forget(&fingerprint).unwrap();
    store.put(&fingerprint, &data()).unwrap();
    check(
        "outside_request_uses_memory",
        memory.get(&fingerprint).unwrap() == Some(data()),
        "not in memory",
    );
}
