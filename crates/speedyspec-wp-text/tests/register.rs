//! `register_text_filters`: formatting's default callbacks, as `default-filters.php` registers them.

use std::rc::Rc;

use speedyspec_wp_hooks::{ArrayKey, CallbackId, HasFilterResult, HookApi, HookName, HookValue};
use speedyspec_wp_text::{FilterContext, register_text_filters};

#[path = "common/api.rs"]
mod api;

/// `(hook, function, priority, admin only)` for every `add_filter()` line in WordPress's
/// `wp-includes/default-filters.php` that names a function this crate ports, in file order.
const EXPECTED: &[(&str, &str, i64, i64, bool)] = &[
    ("pre_term_name", "sanitize_text_field", 10, 1, false),
    ("pre_comment_author_name", "sanitize_text_field", 10, 1, false),
    ("pre_link_name", "sanitize_text_field", 10, 1, false),
    ("pre_link_target", "sanitize_text_field", 10, 1, false),
    ("pre_link_rel", "sanitize_text_field", 10, 1, false),
    ("pre_user_display_name", "sanitize_text_field", 10, 1, false),
    ("pre_user_first_name", "sanitize_text_field", 10, 1, false),
    ("pre_user_last_name", "sanitize_text_field", 10, 1, false),
    ("pre_user_nickname", "sanitize_text_field", 10, 1, false),
    ("term_name", "sanitize_text_field", 10, 1, true),
    ("comment_author_name", "sanitize_text_field", 10, 1, true),
    ("link_name", "sanitize_text_field", 10, 1, true),
    ("link_target", "sanitize_text_field", 10, 1, true),
    ("link_rel", "sanitize_text_field", 10, 1, true),
    ("user_display_name", "sanitize_text_field", 10, 1, true),
    ("user_first_name", "sanitize_text_field", 10, 1, true),
    ("user_last_name", "sanitize_text_field", 10, 1, true),
    ("user_nickname", "sanitize_text_field", 10, 1, true),
    ("pre_comment_author_email", "sanitize_email", 10, 1, false),
    ("pre_user_email", "sanitize_email", 10, 1, false),
    ("comment_author_email", "sanitize_email", 10, 1, false),
    ("user_email", "sanitize_email", 10, 1, false),
    ("pre_comment_author_url", "wp_strip_all_tags", 10, 1, false),
    ("pre_comment_author_url", "sanitize_url", 10, 1, false),
    ("pre_user_url", "wp_strip_all_tags", 10, 1, false),
    ("pre_user_url", "sanitize_url", 10, 1, false),
    ("pre_link_url", "wp_strip_all_tags", 10, 1, false),
    ("pre_link_url", "sanitize_url", 10, 1, false),
    ("pre_link_image", "wp_strip_all_tags", 10, 1, false),
    ("pre_link_image", "sanitize_url", 10, 1, false),
    ("pre_link_rss", "wp_strip_all_tags", 10, 1, false),
    ("pre_link_rss", "sanitize_url", 10, 1, false),
    ("pre_post_guid", "wp_strip_all_tags", 10, 1, false),
    ("pre_post_guid", "sanitize_url", 10, 1, false),
    ("user_url", "wp_strip_all_tags", 10, 1, true),
    ("user_url", "esc_url", 10, 1, false),
    ("link_url", "wp_strip_all_tags", 10, 1, true),
    ("link_url", "esc_url", 10, 1, false),
    ("link_image", "wp_strip_all_tags", 10, 1, true),
    ("link_image", "esc_url", 10, 1, false),
    ("link_rss", "wp_strip_all_tags", 10, 1, true),
    ("link_rss", "esc_url", 10, 1, false),
    ("comment_url", "wp_strip_all_tags", 10, 1, true),
    ("comment_url", "esc_url", 10, 1, false),
    ("post_guid", "wp_strip_all_tags", 10, 1, true),
    ("post_guid", "esc_url", 10, 1, false),
    ("pre_term_slug", "sanitize_title", 10, 1, false),
    ("pre_post_type", "sanitize_key", 10, 1, false),
    ("pre_post_status", "sanitize_key", 10, 1, false),
    ("pre_post_comment_status", "sanitize_key", 10, 1, false),
    ("pre_post_ping_status", "sanitize_key", 10, 1, false),
    ("pre_post_mime_type", "sanitize_mime_type", 10, 1, false),
    ("post_mime_type", "sanitize_mime_type", 10, 1, false),
    ("content_save_pre", "convert_invalid_entities", 10, 1, false),
    ("content_save_pre", "balanceTags", 50, 1, false),
    ("excerpt_save_pre", "convert_invalid_entities", 10, 1, false),
    ("excerpt_save_pre", "balanceTags", 50, 1, false),
    ("comment_save_pre", "convert_invalid_entities", 10, 1, false),
    ("comment_save_pre", "balanceTags", 50, 1, false),
    ("pre_comment_content", "convert_invalid_entities", 10, 1, false),
    ("pre_comment_content", "balanceTags", 50, 1, false),
    ("comment_author", "wptexturize", 10, 1, false),
    ("comment_author", "convert_chars", 10, 1, false),
    ("term_name", "wptexturize", 10, 1, false),
    ("term_name", "convert_chars", 10, 1, false),
    ("link_name", "wptexturize", 10, 1, false),
    ("link_name", "convert_chars", 10, 1, false),
    ("link_description", "wptexturize", 10, 1, false),
    ("link_description", "convert_chars", 10, 1, false),
    ("link_notes", "wptexturize", 10, 1, false),
    ("link_notes", "convert_chars", 10, 1, false),
    ("bloginfo", "wptexturize", 10, 1, false),
    ("bloginfo", "convert_chars", 10, 1, false),
    ("wp_title", "wptexturize", 10, 1, false),
    ("wp_title", "convert_chars", 10, 1, false),
    ("document_title", "wptexturize", 10, 1, false),
    ("document_title", "convert_chars", 10, 1, false),
    ("widget_title", "wptexturize", 10, 1, false),
    ("widget_title", "convert_chars", 10, 1, false),
    ("the_content", "capital_P_dangit", 11, 1, false),
    ("the_title", "capital_P_dangit", 11, 1, false),
    ("wp_title", "capital_P_dangit", 11, 1, false),
    ("document_title", "capital_P_dangit", 11, 1, false),
    ("comment_text", "capital_P_dangit", 31, 1, false),
    ("single_post_title", "wptexturize", 10, 1, false),
    ("single_cat_title", "wptexturize", 10, 1, false),
    ("single_tag_title", "wptexturize", 10, 1, false),
    ("single_month_title", "wptexturize", 10, 1, false),
    ("nav_menu_attr_title", "wptexturize", 10, 1, false),
    ("nav_menu_description", "wptexturize", 10, 1, false),
    ("term_description", "wptexturize", 10, 1, false),
    ("term_description", "convert_chars", 10, 1, false),
    ("term_description", "wpautop", 10, 1, false),
    ("term_description", "shortcode_unautop", 10, 1, false),
    ("get_the_post_type_description", "wptexturize", 10, 1, false),
    ("get_the_post_type_description", "convert_chars", 10, 1, false),
    ("get_the_post_type_description", "wpautop", 10, 1, false),
    ("get_the_post_type_description", "shortcode_unautop", 10, 1, false),
    ("term_name_rss", "convert_chars", 10, 1, false),
    ("the_title", "wptexturize", 10, 1, false),
    ("the_title", "convert_chars", 10, 1, false),
    ("the_content", "wptexturize", 10, 1, false),
    ("the_content", "convert_smilies", 20, 1, false),
    ("the_content", "wpautop", 10, 1, false),
    ("the_content", "shortcode_unautop", 10, 1, false),
    ("the_excerpt", "wptexturize", 10, 1, false),
    ("the_excerpt", "convert_smilies", 10, 1, false),
    ("the_excerpt", "convert_chars", 10, 1, false),
    ("the_excerpt", "wpautop", 10, 1, false),
    ("the_excerpt", "shortcode_unautop", 10, 1, false),
    ("the_post_thumbnail_caption", "wptexturize", 10, 1, false),
    ("the_post_thumbnail_caption", "convert_smilies", 10, 1, false),
    ("the_post_thumbnail_caption", "convert_chars", 10, 1, false),
    ("comment_text", "wptexturize", 10, 1, false),
    ("comment_text", "convert_chars", 10, 1, false),
    ("comment_text", "make_clickable", 9, 1, false),
    ("comment_text", "force_balance_tags", 25, 1, false),
    ("comment_text", "convert_smilies", 20, 1, false),
    ("comment_text", "wpautop", 30, 1, false),
    ("comment_excerpt", "convert_chars", 10, 1, false),
    ("list_cats", "wptexturize", 10, 1, false),
    ("widget_text", "balanceTags", 10, 1, false),
    ("widget_text_content", "capital_P_dangit", 11, 1, false),
    ("widget_text_content", "wptexturize", 10, 1, false),
    ("widget_text_content", "convert_smilies", 20, 1, false),
    ("widget_text_content", "wpautop", 10, 1, false),
    ("widget_text_content", "shortcode_unautop", 10, 1, false),
    ("the_title_rss", "ent2ncr", 8, 1, false),
    ("the_content_rss", "ent2ncr", 8, 1, false),
    ("the_excerpt_rss", "convert_chars", 10, 1, false),
    ("the_excerpt_rss", "ent2ncr", 8, 1, false),
    ("comment_author_rss", "ent2ncr", 8, 1, false),
    ("comment_text_rss", "ent2ncr", 8, 1, false),
    ("bloginfo_rss", "ent2ncr", 8, 1, false),
    ("the_author", "ent2ncr", 8, 1, false),
    ("the_guid", "esc_url", 10, 1, false),
    ("pre_kses", "wp_pre_kses_less_than", 10, 1, false),
    ("sanitize_title", "sanitize_title_with_dashes", 10, 3, false),
    ("pre_comment_content", "wp_rel_ugc", 15, 1, false),
    ("comment_email", "antispambot", 10, 1, false),
    ("the_excerpt_embed", "wptexturize", 10, 1, false),
    ("the_excerpt_embed", "convert_chars", 10, 1, false),
    ("the_excerpt_embed", "wpautop", 10, 1, false),
    ("the_excerpt_embed", "shortcode_unautop", 10, 1, false),
];

/// `has_filter( $hook, $function )`: the priority, or `None` for false.
fn registered_priority(hooks: &HookApi, hook: &str, function: &str) -> Option<i64> {
    match hooks.has_filter(&HookName::from(hook), Some(&CallbackId(Some(function.into()))), None).unwrap() {
        HasFilterResult::At(ArrayKey::Int(p)) => Some(p),
        HasFilterResult::No => None,
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn registers_like_default_filters() {
    let h = api::harness();
    register_text_filters(&h.api, &h.hooks, FilterContext { admin: false }).unwrap();
    for (hook, function, priority, _, admin_only) in EXPECTED {
        let answer = registered_priority(&h.hooks, hook, function);
        let expected = if *admin_only && !EXPECTED.iter().any(|(h2, f2, _, _, a2)| h2 == hook && f2 == function && !a2) {
            None
        } else {
            Some(*priority)
        };
        assert_eq!(answer, expected, "{hook} / {function}");
    }
}

#[test]
fn admin_adds_its_lines() {
    let h = api::harness();
    register_text_filters(&h.api, &h.hooks, FilterContext { admin: true }).unwrap();
    for (hook, function, priority, _, _) in EXPECTED {
        assert_eq!(registered_priority(&h.hooks, hook, function), Some(*priority), "{hook} / {function}");
    }
}

#[test]
fn the_content_chain_runs() {
    let h = api::harness();
    register_text_filters(&h.api, &h.hooks, FilterContext { admin: false }).unwrap();
    let input = "'a' Wordpress\n\n[gallery]\n\nb -- c";
    let out = h.hooks.apply_filters(&HookName::from("the_content"), HookValue::Str(input.into()), Vec::new()).unwrap();

    let fresh = api::harness();
    let api = Rc::clone(&fresh.api);
    let HookValue::Str(texturized) = api.wptexturize(&HookValue::Str(input.into()), false).unwrap() else {
        panic!("a string")
    };
    let autop = api.wpautop(texturized.as_bytes(), true).unwrap();
    let unautop = api.shortcode_unautop(&autop).unwrap();
    let expected = api.capital_p_dangit(&unautop).unwrap().into_owned();
    assert_eq!(out, HookValue::Str(String::from_utf8(expected).unwrap()));
}

#[test]
fn registers_plan_4_rows() {
    let h = api::harness();
    register_text_filters(&h.api, &h.hooks, FilterContext { admin: false }).unwrap();
    let title = h
        .hooks
        .apply_filters(&HookName::from("sanitize_title"), HookValue::Str("A B".into()), vec![HookValue::Str("A B".into()), HookValue::Str("save".into())])
        .unwrap();
    assert_eq!(*title.read(), HookValue::Str("a-b".into()));
    let email = h.hooks.apply_filters(&HookName::from("pre_user_email"), HookValue::Str(" a@b.co ".into()), Vec::new()).unwrap();
    assert_eq!(*email.read(), HookValue::Str("a@b.co".into()));
    let content = h.hooks.apply_filters(&HookName::from("pre_kses"), HookValue::Str("a < b".into()), Vec::new()).unwrap();
    assert_eq!(*content.read(), HookValue::Str("a &lt; b".into()));
}

#[test]
fn callback_errors_keep_their_payload() {
    use speedyspec_wp_hooks::{Callback, HookError};
    let h = api::harness();
    register_text_filters(&h.api, &h.hooks, FilterContext { admin: false }).unwrap();
    let boom = Callback::rust("boom", |_: &[HookValue]| -> Result<HookValue, HookError> {
        Err(HookError { message: "boom".into(), payload: Some(Rc::new(42_i32)) })
    });
    h.hooks.add_filter(&HookName::from("run_wptexturize"), boom, &HookValue::Int(10), 1).unwrap();
    let err = h.hooks.apply_filters(&HookName::from("the_title"), HookValue::Str("a".into()), Vec::new()).unwrap_err();
    let payload = err.payload.expect("the payload is kept");
    assert_eq!(payload.downcast_ref::<i32>(), Some(&42));
}

#[test]
fn default_callback_names_lists_each_function_once() {
    let names = speedyspec_wp_text::default_callback_names();
    assert_eq!(names.len(), 23);
    assert_eq!(&names[..5], &["sanitize_text_field", "sanitize_email", "wp_strip_all_tags", "sanitize_url", "esc_url"]);
    let mut unique = names.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), names.len());
    assert!(names.contains(&"wpautop") && names.contains(&"capital_P_dangit"));
}
