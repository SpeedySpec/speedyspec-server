use std::cell::Cell;
use std::rc::Rc;

use speedyspec_http::Method;
use speedyspec_router::dispatch::ChunkedAlternation;
use speedyspec_router::{CompiledTable, RouteRequest, Router, TableCache, fingerprint_of};
use speedyspec_router_store_memory::MemoryRouteTableStore;
use speedyspec_routing::{GuardAnswer, RouteTableStore, Rule, RuleGuard};

struct AcceptAll;

impl RuleGuard for AcceptAll {
    fn check(&self, _rule: &Rule, _captures: &[Option<String>]) -> GuardAnswer {
        GuardAnswer::Accept
    }
}

fn router(paths: &[&'static str]) -> Router<&'static str> {
    let mut router = Router::new();
    for path in paths {
        router.get(path, "h");
    }
    router
}

fn cache(store: Rc<MemoryRouteTableStore>) -> TableCache {
    TableCache::new(store, Rc::new(ChunkedAlternation::default()))
}

#[test]
fn unchanged_fingerprint_skips_build() {
    let store = Rc::new(MemoryRouteTableStore::new());
    let cache = cache(store);
    let r = router(&["/a/{x}"]);
    let builds = Cell::new(0);
    for _ in 0..2 {
        cache
            .table(r.fingerprint().unwrap(), || {
                builds.set(builds.get() + 1);
                r.data()
            })
            .unwrap();
    }
    assert_eq!(builds.get(), 1);
}

#[test]
fn stored_entry_skips_build_in_a_new_worker() {
    let store = Rc::new(MemoryRouteTableStore::new());
    let r = router(&["/a/{x}"]);
    cache(store.clone())
        .table(r.fingerprint().unwrap(), || r.data())
        .unwrap();
    let builds = Cell::new(0);
    cache(store)
        .table(r.fingerprint().unwrap(), || {
            builds.set(builds.get() + 1);
            r.data()
        })
        .unwrap();
    assert_eq!(builds.get(), 0);
}

#[test]
fn changed_fingerprint_rebuilds() {
    let store = Rc::new(MemoryRouteTableStore::new());
    let cache = cache(store.clone());
    let first = router(&["/a/{x}"]);
    let second = router(&["/b/{x}"]);
    let one = cache
        .table(first.fingerprint().unwrap(), || first.data())
        .unwrap();
    let two = cache
        .table(second.fingerprint().unwrap(), || second.data())
        .unwrap();
    assert!(!Rc::ptr_eq(&one, &two));
    assert!(store.get(&second.fingerprint().unwrap()).unwrap().is_some());
}

#[test]
fn stored_data_compiles_like_direct() {
    let paths: Vec<String> = (0..40).map(|i| format!("/p{i}/{{x}}/{{y?}}")).collect();
    let mut r: Router<&str> = Router::new();
    for path in &paths {
        r.get(path, "h");
    }
    r.get("/static", "s");
    let data = r.data().unwrap();
    let round = speedyspec_routing::RouteData::from_bytes(&data.to_bytes()).unwrap();
    assert_eq!(fingerprint_of(&round), fingerprint_of(&data));
    let direct = CompiledTable::compile(&data, &ChunkedAlternation::default()).unwrap();
    let stored = CompiledTable::compile(&round, &ChunkedAlternation::default()).unwrap();
    for i in 0..200 {
        let path = format!("/p{}/v{i}{}", i % 45, if i % 2 == 0 { "/z" } else { "" });
        let request = RouteRequest {
            method: Method::Get,
            path,
            wordpress: None,
        };
        assert_eq!(
            direct.resolve(&request, &AcceptAll, &[], false),
            stored.resolve(&request, &AcceptAll, &[], false)
        );
    }
}
