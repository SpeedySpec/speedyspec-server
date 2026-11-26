mod support;

use speedyspec_wp_rewrite::RewriteState;
use support::fixture;

fn init(structure: &str) -> (support::Fixture, RewriteState) {
    let f = fixture(structure);
    let mut state = RewriteState::default();
    f.service.init(&mut state).unwrap();
    (f, state)
}

#[test]
fn init_reads_options() {
    let (_, state) = init("/%postname%/");
    assert_eq!(state.permalink_structure, "/%postname%/");
    assert_eq!(state.front, "/");
    assert!(state.use_trailing_slashes);
    assert!(state.use_verbose_page_rules);

    let (_, state) = init("/archives/%post_id%");
    assert_eq!(state.front, "/archives/");
    assert!(!state.use_trailing_slashes);
    assert!(!state.use_verbose_page_rules);
}

#[test]
fn index_permalinks_set_root() {
    let (f, state) = init("/index.php/%postname%/");
    assert_eq!(state.root, "index.php/");
    assert!(f.service.using_index_permalinks(&state));
    assert!(!f.service.using_mod_rewrite_permalinks(&state));
    let (f, state) = init("");
    assert!(!f.service.using_permalinks(&state));
}

#[test]
fn date_permastruct_avoids_clash() {
    let (f, mut state) = init("/%post_id%/%postname%/");
    assert_eq!(
        f.service.get_date_permastruct(&mut state).as_deref(),
        Some("/date/%year%/%monthnum%/%day%")
    );
    let (f, mut state) = init("/%year%/%postname%/");
    assert_eq!(
        f.service.get_date_permastruct(&mut state).as_deref(),
        Some("/%year%/%monthnum%/%day%")
    );
    assert_eq!(
        f.service.get_year_permastruct(&mut state).as_deref(),
        Some("/%year%/")
    );
    assert_eq!(
        f.service.get_month_permastruct(&mut state).as_deref(),
        Some("/%year%/%monthnum%/")
    );
}

#[test]
fn plain_permalinks_have_no_structures_then_cache_empty() {
    let (f, mut state) = init("");
    assert_eq!(f.service.get_author_permastruct(&mut state), None);
    assert_eq!(
        f.service.get_author_permastruct(&mut state).as_deref(),
        Some("")
    );
}

#[test]
fn other_permastructs() {
    let (f, mut state) = init("/blog/%postname%/");
    assert_eq!(
        f.service.get_author_permastruct(&mut state).as_deref(),
        Some("/blog/author/%author%")
    );
    assert_eq!(
        f.service.get_search_permastruct(&mut state).as_deref(),
        Some("search/%search%")
    );
    assert_eq!(
        f.service.get_page_permastruct(&mut state).as_deref(),
        Some("%pagename%")
    );
    assert_eq!(
        f.service.get_feed_permastruct(&mut state).as_deref(),
        Some("feed/%feed%")
    );
    assert_eq!(
        f.service
            .get_comment_feed_permastruct(&mut state)
            .as_deref(),
        Some("comments/feed/%feed%")
    );
}

#[test]
fn set_permalink_structure_fires_changed_once() {
    let (f, mut state) = init("/%postname%/");
    support::record(&f, "permalink_structure_changed");
    f.service
        .set_permalink_structure(&mut state, "/%year%/%postname%/")
        .unwrap();
    f.service
        .set_permalink_structure(&mut state, "/%year%/%postname%/")
        .unwrap();
    assert_eq!(support::hooks_run(&f), vec!["permalink_structure_changed"]);
    assert_eq!(
        f.options.get("permalink_structure").as_deref(),
        Some("/%year%/%postname%/")
    );
    assert_eq!(state.permalink_structure, "/%year%/%postname%/");
}

#[test]
fn category_base_applies() {
    let (f, mut state) = init("/%postname%/");
    f.service.set_category_base(&mut state, "topics").unwrap();
    assert_eq!(f.options.get("category_base").as_deref(), Some("topics"));
    f.service.set_tag_base(&mut state, "labels").unwrap();
    assert_eq!(f.options.get("tag_base").as_deref(), Some("labels"));
}

#[test]
fn init_reads_the_random_base() {
    let f = fixture("/%postname%/");
    f.options.set("wp_random_base", "surprise");
    let mut state = RewriteState::default();
    f.service.init(&mut state).unwrap();
    assert_eq!(state.random_base, "surprise");
    assert_eq!(f.service.get_random_base(&state).unwrap(), "surprise");
}

#[test]
fn the_random_base_defaults_to_its_translation() {
    struct Translated(&'static str);
    impl speedyspec_wp_rewrite::SiteTranslations for Translated {
        fn random_base(&self) -> Result<String, speedyspec_wp_rewrite::RewriteError> {
            Ok(self.0.into())
        }
    }
    let (f, state) = init("/%postname%/");
    assert_eq!(f.service.get_random_base(&state).unwrap(), "random");
    for (translation, base) in [("zufall", "zufall"), ("", "random")] {
        let mut deps = f.service.deps().clone();
        deps.translations = std::rc::Rc::new(Translated(translation));
        let service = speedyspec_wp_rewrite::RewriteService::new(deps);
        assert_eq!(service.get_random_base(&state).unwrap(), base);
    }
}

#[test]
fn set_random_base_updates_and_reinitialises() {
    let (f, mut state) = init("/%postname%/");
    state.author_structure = Some("cached".into());
    f.service.set_random_base(&mut state, "lucky").unwrap();
    assert_eq!(f.options.get("wp_random_base").as_deref(), Some("lucky"));
    assert_eq!(state.random_base, "lucky");
    assert_eq!(state.author_structure, None);
    state.author_structure = Some("cached".into());
    f.service.set_random_base(&mut state, "lucky").unwrap();
    assert_eq!(state.author_structure.as_deref(), Some("cached"));
}

#[test]
fn post_permalinks_share_the_random_base_only_at_the_root() {
    for (structure, shares) in [
        ("/%postname%/", true),
        ("/%postname%", true),
        ("/index.php/%postname%/", true),
        ("/blog/%postname%/", false),
        ("/%year%/%postname%/", false),
        ("", false),
    ] {
        let (f, state) = init(structure);
        assert_eq!(
            f.service.post_permalinks_share_random_base(&state),
            shares,
            "{structure}"
        );
    }
}
