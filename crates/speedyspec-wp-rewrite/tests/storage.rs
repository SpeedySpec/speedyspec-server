mod support;

use speedyspec_wp_rewrite::{RewriteState, RewriteStateSource};
use support::fixture;

fn state(f: &support::Fixture) -> RewriteState {
    let mut s = RewriteState::default();
    f.service.init(&mut s).unwrap();
    s
}

#[test]
fn wp_rewrite_rules_reads_the_option() {
    let f = fixture("/%postname%/");
    let mut s = state(&f);
    let stored = vec![speedyspec_routing::Rule {
        pattern: "a$".into(),
        query: "index.php?a=1".into(),
    }];
    use speedyspec_wp_rewrite::RewriteOptions;
    f.options.set_rules(&stored).unwrap();
    assert_eq!(
        f.service
            .wp_rewrite_rules(&mut s, f.source.as_ref())
            .unwrap(),
        stored
    );
}

#[test]
fn missing_option_is_generated_and_saved() {
    let f = fixture("/%postname%/");
    let mut s = state(&f);
    let rules = f
        .service
        .wp_rewrite_rules(&mut s, f.source.as_ref())
        .unwrap();
    assert!(!rules.is_empty());
    assert_eq!(f.options.stored_rules(), Some(rules));
    assert_eq!(s.matches, "matches");
}

#[test]
fn refresh_before_wp_loaded_defers_save() {
    let f = fixture("/%postname%/");
    f.loaded.set(false);
    let mut s = state(&f);
    let rules = f
        .service
        .wp_rewrite_rules(&mut s, f.source.as_ref())
        .unwrap();
    assert!(!rules.is_empty());
    assert_eq!(f.options.stored_rules(), None);
    f.loaded.set(true);
    f.hooks
        .do_action(
            &speedyspec_wp_hooks_contracts::HookName::from("wp_loaded"),
            vec![],
        )
        .unwrap();
    assert_eq!(f.options.stored_rules(), Some(rules));
}

#[test]
fn flush_hard_fires_filter() {
    let f = fixture("/%postname%/");
    support::record(&f, "flush_rewrite_rules_hard");
    let mut s = state(&f);
    f.service
        .flush_rules(&mut s, f.source.as_ref(), false)
        .unwrap();
    assert!(support::hooks_run(&f).is_empty());
    f.service
        .flush_rules(&mut s, f.source.as_ref(), true)
        .unwrap();
    assert_eq!(support::hooks_run(&f), vec!["flush_rewrite_rules_hard"]);
    assert!(f.options.stored_rules().is_some());
}

#[test]
fn flush_before_wp_loaded_runs_on_wp_loaded_once_with_hard_kept() {
    let f = fixture("/%postname%/");
    support::record(&f, "flush_rewrite_rules_hard");
    f.loaded.set(false);
    let mut s = state(&f);
    f.source.store(&s);
    f.service
        .flush_rules(&mut s, f.source.as_ref(), true)
        .unwrap();
    f.service
        .flush_rules(&mut s, f.source.as_ref(), false)
        .unwrap();
    assert_eq!(f.options.stored_rules(), None);
    f.loaded.set(true);
    f.hooks
        .do_action(
            &speedyspec_wp_hooks_contracts::HookName::from("wp_loaded"),
            vec![],
        )
        .unwrap();
    assert!(f.options.stored_rules().is_some());
    assert_eq!(support::hooks_run(&f), vec!["flush_rewrite_rules_hard"]);
}

#[test]
fn plain_permalinks_save_an_empty_value() {
    let f = fixture("");
    let mut s = state(&f);
    assert!(
        f.service
            .wp_rewrite_rules(&mut s, f.source.as_ref())
            .unwrap()
            .is_empty()
    );
    // WordPress stores an empty string, not an empty array: rewrite_rules() leaves $this->rules as ''.
    assert_eq!(f.options.get("rewrite_rules").as_deref(), Some(""));
    assert_eq!(f.options.stored_rules(), None);
}

/// `refresh_rewrite_rules()` defers `flush_rules` to `wp_loaded` without setting `$do_hard_later`, and
/// `do_action( 'wp_loaded' )` passes `''`: the deferred flush is soft.
#[test]
fn flush_deferred_by_refresh_is_soft() {
    let f = fixture("/%postname%/");
    support::record(&f, "flush_rewrite_rules_hard");
    f.loaded.set(false);
    let mut s = state(&f);
    f.service
        .wp_rewrite_rules(&mut s, f.source.as_ref())
        .unwrap();
    f.loaded.set(true);
    f.hooks
        .do_action(
            &speedyspec_wp_hooks_contracts::HookName::from("wp_loaded"),
            vec![],
        )
        .unwrap();
    assert!(f.options.stored_rules().is_some());
    assert!(support::hooks_run(&f).is_empty());
}

/// `unset()` on a static leaves its value for the next call, so `$do_hard_later` decides every later flush.
#[test]
fn deferred_hard_decides_later_flushes_too() {
    let f = fixture("/%postname%/");
    support::record(&f, "flush_rewrite_rules_hard");
    f.loaded.set(false);
    let mut s = state(&f);
    f.source.store(&s);
    f.service
        .flush_rules(&mut s, f.source.as_ref(), true)
        .unwrap();
    f.loaded.set(true);
    f.hooks
        .do_action(
            &speedyspec_wp_hooks_contracts::HookName::from("wp_loaded"),
            vec![],
        )
        .unwrap();
    let mut s = f.source.load().unwrap();
    f.service
        .flush_rules(&mut s, f.source.as_ref(), false)
        .unwrap();
    assert_eq!(
        support::hooks_run(&f),
        vec!["flush_rewrite_rules_hard", "flush_rewrite_rules_hard"]
    );
}
