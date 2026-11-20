use speedyspec_wp_options_memory::MemorySitemetaRepository;
use speedyspec_wp_options_memory::suites::sitemeta_repository_suite;

#[test]
fn memory_sitemeta_suite() {
    sitemeta_repository_suite(&MemorySitemetaRepository::new());
}
