//! The native functions behind `WP_Theme_JSON_Resolver` and the global styles functions.

use std::rc::Rc;

use speedyspec_hooks::Value;
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_php::native::{self, NativeError};
use speedyspec_wp_theme_json::global_styles::GlobalStylesApi;
use speedyspec_wp_theme_json_contracts::ThemeJsonError;
use speedyspec_wp_theme_json_contracts::model::ThemeJson;

use crate::arena::HandleArena;

const NATIVE: &str = "SpeedySpec\\WpThemeJson\\Native\\";

type R = Result<Value, NativeError>;

fn arg(args: &[Value], i: usize) -> Value {
    args.get(i).cloned().unwrap_or(Value::Null)
}

fn fail(e: ThemeJsonError) -> NativeError {
    NativeError::throw("Error", e.to_string())
}

fn handle(doc: ThemeJson) -> R {
    Ok(Value::Int(HandleArena::insert(doc)))
}

fn doc(args: &[Value]) -> Result<Rc<ThemeJson>, NativeError> {
    let h = match arg(args, 0) {
        Value::Int(h) => h,
        _ => -1,
    };
    HandleArena::get(h)
        .ok_or_else(|| NativeError::throw("Error", "WP_Theme_JSON: the document is gone"))
}

fn text(bytes: Vec<u8>) -> Value {
    match String::from_utf8(bytes) {
        Ok(s) => Value::Str(s),
        Err(e) => Value::Bytes(e.into_bytes()),
    }
}

/// Every resolver and global styles function, over `styles`.
pub(crate) fn functions(styles: Rc<GlobalStylesApi>) -> Vec<FunctionBuilder<'static>> {
    type Body = Box<dyn Fn(&GlobalStylesApi, &[Value]) -> R>;
    let f = |name: &str, body: Body| {
        let styles = styles.clone();
        native::function(&format!("{NATIVE}{name}"), move |args| body(&styles, args))
    };
    vec![
        f(
            "resolver_core_data",
            Box::new(|g, _| handle(g.resolver().get_core_data().map_err(fail)?)),
        ),
        f(
            "resolver_block_data",
            Box::new(|g, _| handle(g.resolver().get_block_data().map_err(fail)?)),
        ),
        f(
            "resolver_theme_data",
            Box::new(|g, a| {
                let with_supports = matches!(arg(a, 1), Value::Bool(true));
                handle(
                    g.resolver()
                        .get_theme_data(&arg(a, 0), with_supports)
                        .map_err(fail)?,
                )
            }),
        ),
        f(
            "resolver_user_data",
            Box::new(|g, _| handle(g.resolver().get_user_data().map_err(fail)?)),
        ),
        f(
            "resolver_merged_data",
            Box::new(|g, a| handle(g.resolver().get_merged_data(&arg(a, 0)).map_err(fail)?)),
        ),
        f(
            "resolver_user_post_id",
            Box::new(|g, _| Ok(g.resolver().get_user_global_styles_post_id())),
        ),
        f(
            "resolver_theme_has_support",
            Box::new(|g, _| Ok(Value::Bool(g.resolver().theme_has_support()))),
        ),
        f(
            "resolver_fields_to_translate",
            Box::new(|g, _| Ok(g.resolver().get_fields_to_translate())),
        ),
        f(
            "resolver_clean_cached_data",
            Box::new(|g, _| {
                g.resolver().clean_cached_data();
                Ok(Value::Null)
            }),
        ),
        f(
            "resolver_style_variations",
            Box::new(|g, a| {
                let scope = String::from_utf8_lossy(&speedyspec_wp_theme_json::php::to_php_string(
                    &arg(a, 0),
                ))
                .into_owned();
                Ok(g.resolver().get_style_variations(&scope))
            }),
        ),
        f(
            "resolver_resolved_theme_uris",
            Box::new(|g, a| Ok(g.resolver().get_resolved_theme_uris(Some(&*doc(a)?)))),
        ),
        f(
            "resolver_resolve_theme_file_uris",
            Box::new(|g, a| handle(g.resolver().resolve_theme_file_uris(&*doc(a)?))),
        ),
        f(
            "global_settings",
            Box::new(|g, a| g.get_global_settings(&arg(a, 0), &arg(a, 1)).map_err(fail)),
        ),
        f(
            "global_styles",
            Box::new(|g, a| g.get_global_styles(&arg(a, 0), &arg(a, 1)).map_err(fail)),
        ),
        f(
            "global_stylesheet",
            Box::new(|g, a| Ok(text(g.get_global_stylesheet(&arg(a, 0)).map_err(fail)?))),
        ),
        f(
            "block_name_from_theme_json_path",
            Box::new(|_, a| {
                Ok(GlobalStylesApi::get_block_name_from_theme_json_path(&arg(
                    a, 0,
                )))
            }),
        ),
        f(
            "clean_theme_json_cache",
            Box::new(|g, _| {
                g.clean_theme_json_cache();
                Ok(Value::Null)
            }),
        ),
        f(
            "theme_directory_pattern_slugs",
            Box::new(|g, _| g.get_theme_directory_pattern_slugs().map_err(fail)),
        ),
        f(
            "theme_data_custom_templates",
            Box::new(|g, _| g.get_theme_data_custom_templates().map_err(fail)),
        ),
        f(
            "theme_data_template_parts",
            Box::new(|g, _| g.get_theme_data_template_parts().map_err(fail)),
        ),
    ]
}
