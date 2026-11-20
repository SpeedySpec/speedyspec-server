use speedyspec_cache_conformance::caching_store_suite;
use speedyspec_cache_memory::MemoryCache;

#[test]
fn memory_cache_conformance() {
    caching_store_suite(&MemoryCache::new());
}
