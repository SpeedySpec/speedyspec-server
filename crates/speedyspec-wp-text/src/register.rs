//! `register_text_filters`: formatting's share of `default-filters.php`, as Rust callbacks.

use std::rc::Rc;

use speedyspec_wp_hooks::{Callback, HookApi, HookError, HookName, HookValue};
use speedyspec_wp_text_contracts::TextError;

use crate::api::TextApi;
use crate::balance::force_balance_tags;
use crate::small::convert_invalid_entities;
use crate::strings::{bytes_value, to_php_string};
use crate::title::{SanitizeContext, sanitize_title_with_dashes};

/// Where the filters are registered: `default-filters.php` adds a few only in the admin.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FilterContext {
    /// Whether `is_admin()` is true.
    pub admin: bool,
}

/// `(hook, function, priority, accepted_args, admin only)` for every `add_filter()` line in WordPress 7.1.5's
/// `wp-includes/default-filters.php` that names a function this crate ports, in file order.
const REGISTRATIONS: &[(&str, &str, i64, i64, bool)] = &[
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

/// The distinct functions `default-filters.php` adds as formatting callbacks, in first-seen order: the names a
/// language bridge may run as Rust callbacks when they are added by name.
///
/// @return
///     Each function name once.
pub fn default_callback_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = Vec::new();
    for &(_, function, ..) in REGISTRATIONS {
        if !names.contains(&function) {
            names.push(function);
        }
    }
    names
}

/// The hook error of a callback's failure, keeping its payload so a language bridge can rethrow the original
/// (a PHP exception object) or stay silent while PHP unwinds.
///
/// @param e
///     The formatting error.
/// @return
///     The hook error, with the same message and payload.
pub fn hook_error(e: TextError) -> HookError {
    HookError { message: e.message, payload: e.payload }
}

/// The first argument as a PHP string, or the error PHP 8 raises for a non-scalar.
fn text_arg(function: &str, args: &[HookValue]) -> Result<Vec<u8>, HookError> {
    to_php_string(args.first().unwrap_or(&HookValue::Null))
        .ok_or_else(|| HookError::message(format!("{function}(): Argument #1 must be of type string")))
}

/// The Rust callback for one of formatting's functions.
fn callback(api: &Rc<TextApi>, function: &'static str) -> Callback {
    let api = api.clone();
    Callback::rust(function, move |args: &[HookValue]| -> Result<HookValue, HookError> {
        let first = args.first().cloned().unwrap_or(HookValue::Null);
        Ok(match function {
            "wptexturize" => api.wptexturize(&first, false).map_err(hook_error)?,
            "wp_strip_all_tags" => bytes_value(api.wp_strip_all_tags(&first, false).map_err(hook_error)?),
            "ent2ncr" => api.ent2ncr(&text_arg(function, args)?).map_err(hook_error)?,
            "sanitize_text_field" => api.sanitize_text_field(&first).map_err(hook_error)?,
            "sanitize_key" => api.sanitize_key(&first).map_err(hook_error)?,
            "sanitize_title" => api.sanitize_title(&first, b"", &SanitizeContext::Save).map_err(hook_error)?,
            "sanitize_email" => api.sanitize_email(&text_arg(function, args)?).map_err(hook_error)?,
            "sanitize_url" => api.sanitize_url(&text_arg(function, args)?, None).map_err(hook_error)?,
            "esc_url" => api.esc_url(&text_arg(function, args)?, None, b"display").map_err(hook_error)?,
            "sanitize_mime_type" => api.sanitize_mime_type(&text_arg(function, args)?).map_err(hook_error)?,
            "sanitize_title_with_dashes" => {
                let context = args.get(2).and_then(to_php_string).unwrap_or_else(|| b"display".to_vec());
                bytes_value(sanitize_title_with_dashes(&text_arg(function, args)?, &SanitizeContext::from_bytes(&context)))
            }
            other => {
                let text = text_arg(other, args)?;
                bytes_value(match other {
                    "wpautop" => api.wpautop(&text, true).map_err(hook_error)?.into_owned(),
                    "shortcode_unautop" => api.shortcode_unautop(&text).map_err(hook_error)?.into_owned(),
                    "convert_chars" => api.convert_chars(&text, &HookValue::Null).map_err(hook_error)?.into_owned(),
                    "convert_invalid_entities" => convert_invalid_entities(&text).into_owned(),
                    "capital_P_dangit" => api.capital_p_dangit(&text).map_err(hook_error)?.into_owned(),
                    "balanceTags" => api.balance_tags(&text, false).map_err(hook_error)?.into_owned(),
                    "force_balance_tags" => force_balance_tags(&text).into_owned(),
                    "convert_smilies" => api.convert_smilies(&text).map_err(hook_error)?.into_owned(),
                    "make_clickable" => api.make_clickable(&text).map_err(hook_error)?,
                    "wp_pre_kses_less_than" => api.wp_pre_kses_less_than(&text).map_err(hook_error)?,
                    "wp_rel_ugc" => api.wp_rel_ugc(&text).map_err(hook_error)?,
                    "antispambot" => api.antispambot(&text, 0).map_err(hook_error)?,
                    unknown => unreachable!("no callback for {unknown}"),
                })
            }
        })
    })
}

/// Registers formatting's default callbacks as `default-filters.php` does, with WordPress's ids (the function
/// names), hooks, priorities and accepted arguments, in its order, so `has_filter( 'the_content', 'wpautop' )`
/// answers 10 and `remove_filter()` finds them.
///
/// It is for Rust-only callers and the seam that ports `default-filters.php`. In the embedded PHP,
/// `default-filters.php` itself still registers them by name, and `speedyspec-wp-text-php` stores each such
/// registration as a Rust callback in the same slot.
///
/// @param api
///     The facade the callbacks run.
/// @param hooks
///     Where to register them.
/// @param context
///     Whether the admin-only lines apply.
/// @return
///     Nothing, or the hook store's error.
pub fn register_text_filters(api: &Rc<TextApi>, hooks: &HookApi, context: FilterContext) -> Result<(), HookError> {
    for &(hook, function, priority, accepted_args, admin_only) in REGISTRATIONS {
        if admin_only && !context.admin {
            continue;
        }
        hooks.add_filter(&HookName::from(hook), callback(api, function), &HookValue::Int(priority), accepted_args)?;
    }
    Ok(())
}
