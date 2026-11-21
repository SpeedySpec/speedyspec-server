use speedyspec_router_store_memory::MemoryRouteTableStore;
use speedyspec_routing::conformance;

#[test]
fn memory_store_conforms() {
    conformance::check(&MemoryRouteTableStore::new()).unwrap();
}
