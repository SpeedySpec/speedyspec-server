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

/// A store that answers another table's (valid) data for every fingerprint, as a store with a key bug would.
struct WrongStore(speedyspec_routing::RouteData);

impl speedyspec_routing::RouteTableStore for WrongStore {
    fn get(
        &self,
        _fp: &speedyspec_routing::Fingerprint,
    ) -> Result<Option<speedyspec_routing::RouteData>, speedyspec_routing::StoreError> {
        Ok(Some(self.0.clone()))
    }
    fn put(
        &self,
        _fp: &speedyspec_routing::Fingerprint,
        _data: &speedyspec_routing::RouteData,
    ) -> Result<(), speedyspec_routing::StoreError> {
        Ok(())
    }
    fn forget(
        &self,
        _fp: &speedyspec_routing::Fingerprint,
    ) -> Result<(), speedyspec_routing::StoreError> {
        Ok(())
    }
}

#[test]
fn stored_data_of_another_table_is_a_miss() {
    let other = router(&["/other/{x}"]).data().unwrap();
    let cache = TableCache::new(
        Rc::new(WrongStore(other)),
        Rc::new(ChunkedAlternation::default()),
    );
    let mine = router(&["/mine/{x}"]);
    let table = cache
        .table(mine.fingerprint().unwrap(), || mine.data())
        .unwrap();
    let request = RouteRequest {
        method: Method::Get,
        path: "/mine/1".into(),
        wordpress: None,
    };
    assert!(matches!(
        table.resolve(&request, &AcceptAll, &[], false),
        speedyspec_routing::Resolution::Route(_)
    ));
}

/// App routes and WordPress's rules share one cache: alternating between two tables must not recompile either.
#[test]
fn alternating_tables_are_both_kept() {
    let cache = cache(Rc::new(MemoryRouteTableStore::new()));
    let first = router(&["/a/{x}"]);
    let second = router(&["/b/{x}"]);
    let one = cache
        .table(first.fingerprint().unwrap(), || first.data())
        .unwrap();
    cache
        .table(second.fingerprint().unwrap(), || second.data())
        .unwrap();
    let again = cache
        .table(first.fingerprint().unwrap(), || first.data())
        .unwrap();
    assert!(Rc::ptr_eq(&one, &again));
}

/// Past its capacity the cache drops the least recently used table.
#[test]
fn least_recently_used_table_is_dropped() {
    let cache = TableCache::with_capacity(
        Rc::new(MemoryRouteTableStore::new()),
        Rc::new(ChunkedAlternation::default()),
        2,
    );
    let routers = [
        router(&["/a/{x}"]),
        router(&["/b/{x}"]),
        router(&["/c/{x}"]),
    ];
    let get =
        |r: &Router<&'static str>| cache.table(r.fingerprint().unwrap(), || r.data()).unwrap();
    let a = get(&routers[0]);
    let b = get(&routers[1]);
    get(&routers[0]);
    get(&routers[2]);
    assert!(Rc::ptr_eq(&a, &get(&routers[0])));
    assert!(!Rc::ptr_eq(&b, &get(&routers[1])));
}

#[test]
fn fingerprint_is_sha256_of_route_bytes() {
    let mut r: Router<&str> = Router::new();
    r.get("/p/{x}", "h");
    r.get("/static", "s");
    let data = r.data().unwrap();
    let hex: String = fingerprint_of(&data)
        .0
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    // `shasum -a 256` over `data.to_bytes()`; a change to the route data's byte form changes this pin too.
    assert_eq!(
        hex,
        "dfce5b88602e8edb6bdcc1617407bc9129b8412b6b3ebf34b0a7a38899713828"
    );
}
