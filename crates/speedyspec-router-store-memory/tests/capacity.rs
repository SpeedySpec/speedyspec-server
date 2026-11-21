use speedyspec_router_store_memory::MemoryRouteTableStore;
use speedyspec_routing::{Fingerprint, RouteData, RouteTableStore, Rule};

fn data(n: u8) -> RouteData {
    RouteData {
        statics: vec![],
        dynamics: vec![],
        before_rules: vec![],
        after_rules: vec![Rule {
            pattern: format!("r{n}/?$"),
            query: "index.php".into(),
        }],
        names: vec![],
    }
}

#[test]
fn bounded_store_drops_the_least_recently_used_table() {
    let store = MemoryRouteTableStore::with_capacity(2);
    let (a, b, c) = (
        Fingerprint([1; 32]),
        Fingerprint([2; 32]),
        Fingerprint([3; 32]),
    );
    store.put(&a, &data(1)).unwrap();
    store.put(&b, &data(2)).unwrap();
    store.get(&a).unwrap();
    store.put(&c, &data(3)).unwrap();
    assert_eq!(store.get(&a).unwrap(), Some(data(1)));
    assert_eq!(store.get(&b).unwrap(), None);
    assert_eq!(store.get(&c).unwrap(), Some(data(3)));
}

#[test]
fn default_store_is_bounded() {
    let store = MemoryRouteTableStore::new();
    for n in 0..=MemoryRouteTableStore::DEFAULT_CAPACITY {
        store
            .put(&Fingerprint([n as u8; 32]), &data(n as u8))
            .unwrap();
    }
    assert_eq!(store.get(&Fingerprint([0; 32])).unwrap(), None);
}
