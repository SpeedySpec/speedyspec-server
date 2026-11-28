//! The container keys this area shares with another seam's PHP crate. The post writers resolve the meta tables at
//! the key the meta area binds them at; posts-php keeps its own copy of that key (it may not depend on meta-php), so
//! this test fails the moment the copies drift.

#[test]
fn meta_repositories_key_is_the_one_the_meta_area_binds() {
    assert_eq!(
        speedyspec_wp_posts_php::META_REPOSITORIES,
        speedyspec_wp_meta_php::META_REPOSITORIES
    );
}
