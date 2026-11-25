use std::rc::Rc;

use speedyspec_wp_meta_contracts::conformance::meta_repository_suite;
use speedyspec_wp_meta_contracts::{MetaRepositories, MetaType};
use speedyspec_wp_meta_memory::{MemoryMetaRepositories, MemoryMetaRepository};

#[test]
fn memory_repository_passes_the_suite() {
    meta_repository_suite(&MemoryMetaRepository::new(MetaType::new("post").unwrap()));
    meta_repository_suite(&MemoryMetaRepository::new(MetaType::new("user").unwrap()));
}

#[test]
fn repositories_share_one_per_type() {
    let repositories = MemoryMetaRepositories::new(&["post", "user"]);
    let post = MetaType::new("post").unwrap();
    let first = repositories.for_type(&post).unwrap();
    let second = repositories.for_type(&post).unwrap();
    assert!(Rc::ptr_eq(&first, &second));
    assert_eq!(first.meta_type(), &post);
}

#[test]
fn repositories_unknown_type_is_none() {
    let repositories = MemoryMetaRepositories::new(&["post"]);
    assert!(
        repositories
            .for_type(&MetaType::new("term").unwrap())
            .is_none()
    );
}
