use speedyspec_wp_posts_contracts::conformance::{binary_safe_suite, post_repository_suite};
use speedyspec_wp_posts_memory::MemoryPostRepository;

#[test]
fn memory_repository_passes_the_suite() {
    post_repository_suite(&MemoryPostRepository::new());
}

#[test]
fn memory_repository_passes_the_binary_safe_suite() {
    binary_safe_suite(&MemoryPostRepository::new());
}
