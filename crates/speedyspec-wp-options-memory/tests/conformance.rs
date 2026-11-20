use speedyspec_wp_options::OptionRepository;
use speedyspec_wp_options::conformance::option_repository_suite;
use speedyspec_wp_options_memory::MemoryOptionRepository;

#[test]
fn memory_conformance() {
    let repo = MemoryOptionRepository::new();
    option_repository_suite(&repo);
    assert_eq!(repo.engine(), "memory");
    assert!(repo.statements() > 0);
}
