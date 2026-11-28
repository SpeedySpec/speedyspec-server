//! `new WP_Post_Type( $name, $args )`: the declared properties and `set_props()` (class-wp-post-type.php, 7.2-RC2
//! lines 25-687), after its `register_post_type_args` and `register_{$post_type}_post_type_args` filters, which the
//! caller applies to `args` first.
//!
//! The labels come from [`post_type_labels`] with no `post_type_labels_{$post_type}` callback.
//! [`filter_post_type_labels`] then runs that filter (Observer, through [`HookApi`]) and hands its result to
//! [`apply_filtered_labels`].
//!
//! Approximations, none of them recorded:
//! - a non-array `capabilities` counts as empty (PHP throws a `TypeError` in `array_merge()`);
//! - an array `capability_type` missing index 0 or 1 gives `""` in the capability names (PHP warns), and `null`
//!   as the stored `capability_type` when index 0 is missing;
//! - an int arg key becomes an int property name, as `get_object_vars()` reports it;
//! - [`apply_filtered_labels`] merges against the stored labels, which have already been through the no-filter
//!   merge (`array_merge( $d, $d )`), not against PHP's pre-filter defaults. The two differ only when the labels
//!   have int keys (`labels => 'Books'` gives key `0`, renumbered to `1` by that merge), so such keys are renumbered
//!   once more than PHP would.

use speedyspec_php_serialize::semantics::{
    array_merge, get, is_empty, isset, set, to_array, to_php_string, unset,
};
use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_hooks_contracts::{Count, HookApi};
use speedyspec_wp_posts_contracts::{
    PostError, PostType, PostTypeName, RewriteContext, TitleSanitizer, Translator,
};

use crate::caps::{meta_capabilities, post_type_capabilities};
use crate::hook::{byte_name, hv, pv};
use crate::labels::{LabelSource, merge_filtered_labels, post_type_labels};

/// What `set_props()` reads beyond its arguments.
pub struct PostTypePorts<'a> {
    /// `__()` and `_x()` for the default labels.
    pub translator: &'a dyn Translator,
    /// `is_admin() || get_option( 'permalink_structure' )`.
    pub rewrite: &'a dyn RewriteContext,
    /// `sanitize_title_with_dashes()` for a string `query_var`.
    pub titles: &'a dyn TitleSanitizer,
}

/// A constructed `WP_Post_Type` and the meta capabilities its `get_post_type_capabilities()` call remembered.
#[derive(Debug, Clone)]
pub struct ResolvedPostType {
    /// The object.
    pub post_type: PostType,
    /// `(custom, core)` meta capabilities for `$post_type_meta_caps`; empty unless `map_meta_cap` is truthy.
    pub meta_caps: Vec<(String, &'static str)>,
}

fn s(v: &str) -> PhpValue {
    PhpValue::Str(v.as_bytes().to_vec())
}

fn arr() -> PhpValue {
    PhpValue::Array(Vec::new())
}

/// The declared properties and their defaults (lines 25-482), in declaration order.
fn declared(name: &PostTypeName) -> Vec<(PhpKey, PhpValue)> {
    use PhpValue::{Bool, Null};
    [
        ("name", s(name.as_str())),
        ("label", Null),
        ("labels", Null),
        ("description", s("")),
        ("public", Bool(false)),
        ("hierarchical", Bool(false)),
        ("exclude_from_search", Null),
        ("publicly_queryable", Null),
        ("embeddable", Null),
        ("randomable", Null),
        ("show_ui", Null),
        ("show_in_menu", Null),
        ("show_in_nav_menus", Null),
        ("show_in_admin_bar", Null),
        ("menu_position", Null),
        ("menu_icon", Null),
        ("capability_type", s("post")),
        ("map_meta_cap", Bool(false)),
        ("register_meta_box_cb", Null),
        ("taxonomies", arr()),
        ("has_archive", Bool(false)),
        ("query_var", Null),
        ("can_export", Bool(true)),
        ("delete_with_user", Null),
        ("template", arr()),
        ("template_lock", Bool(false)),
        ("_builtin", Bool(false)),
        ("_edit_link", s("post.php?post=%d")),
        ("cap", Null),
        ("rewrite", Null),
        ("supports", Null),
        ("show_in_rest", Null),
        ("show_in_abilities", Null),
        ("rest_base", Null),
        ("rest_namespace", Null),
        ("rest_controller_class", Null),
        ("rest_controller", Null),
        ("revisions_rest_controller_class", Null),
        ("revisions_rest_controller", Null),
        ("autosave_rest_controller_class", Null),
        ("autosave_rest_controller", Null),
        ("late_route_registration", Null),
    ]
    .into_iter()
    .map(|(key, value)| (PhpKey::Str(key.as_bytes().to_vec()), value))
    .collect()
}

/// `set_props()`' `$defaults` (lines 554-592), in order.
fn defaults() -> Vec<(PhpKey, PhpValue)> {
    use PhpValue::{Bool, Null};
    [
        ("labels", arr()),
        ("description", s("")),
        ("public", Bool(false)),
        ("hierarchical", Bool(false)),
        ("exclude_from_search", Null),
        ("publicly_queryable", Null),
        ("embeddable", Null),
        ("randomable", Null),
        ("show_ui", Null),
        ("show_in_menu", Null),
        ("show_in_nav_menus", Null),
        ("show_in_admin_bar", Null),
        ("menu_position", Null),
        ("menu_icon", Null),
        ("capability_type", s("post")),
        ("capabilities", arr()),
        ("map_meta_cap", Null),
        ("supports", arr()),
        ("register_meta_box_cb", Null),
        ("taxonomies", arr()),
        ("has_archive", Bool(false)),
        ("rewrite", Bool(true)),
        ("query_var", Bool(true)),
        ("can_export", Bool(true)),
        ("delete_with_user", Null),
        ("show_in_rest", Bool(false)),
        ("show_in_abilities", Bool(false)),
        ("rest_base", Bool(false)),
        ("rest_namespace", Bool(false)),
        ("rest_controller_class", Bool(false)),
        ("autosave_rest_controller_class", Bool(false)),
        ("revisions_rest_controller_class", Bool(false)),
        ("late_route_registration", Bool(false)),
        ("template", arr()),
        ("template_lock", Bool(false)),
        ("_builtin", Bool(false)),
        ("_edit_link", s("post.php?post=%d")),
    ]
    .into_iter()
    .map(|(key, value)| (PhpKey::Str(key.as_bytes().to_vec()), value))
    .collect()
}

/// `$args[ $key ]`, null when missing.
fn arg(args: &[(PhpKey, PhpValue)], key: &str) -> PhpValue {
    get(args, key).cloned().unwrap_or(PhpValue::Null)
}

fn is_null(args: &[(PhpKey, PhpValue)], key: &str) -> bool {
    matches!(get(args, key), None | Some(PhpValue::Null))
}

/// `$this->$key = $value`: a property already there (declared or not) is replaced in place, a new one appended.
fn set_property(properties: &mut Vec<(PhpKey, PhpValue)>, key: PhpKey, value: PhpValue) {
    match properties.iter_mut().find(|(k, _)| *k == key) {
        Some(slot) => slot.1 = value,
        None => properties.push((key, value)),
    }
}

/// `new WP_Post_Type( $name, $args )` after set_props()' two args filters, with no post_type_labels_{$name}
/// callback. `Err` carries a failure of the rewrite context (`get_option( 'permalink_structure' )`'s filters) or of
/// the translator (`gettext`), where WordPress's exception would stop `register_post_type()`.
pub fn resolve_post_type(
    name: PostTypeName,
    args: Vec<(PhpKey, PhpValue)>,
    ports: &PostTypePorts<'_>,
) -> Result<ResolvedPostType, PostError> {
    let mut properties = declared(&name);
    let name_value = s(name.as_str());

    let has_edit_link = get(&args, "_edit_link").is_some_and(|v| !is_empty(v));
    let mut args = array_merge(defaults(), args);
    set(&mut args, "name", name_value.clone());

    // The null defaults (lines 598-640), in order.
    for key in ["publicly_queryable", "show_ui", "embeddable"] {
        if is_null(&args, key) {
            let value = arg(&args, "public");
            set(&mut args, key, value);
        }
    }
    // `$args['randomable'] ??= $args['publicly_queryable'] && (bool) $args['has_archive'];`
    if is_null(&args, "randomable") {
        let randomable =
            !is_empty(&arg(&args, "publicly_queryable")) && !is_empty(&arg(&args, "has_archive"));
        set(&mut args, "randomable", PhpValue::Bool(randomable));
    }
    if matches!(get(&args, "rest_namespace"), Some(PhpValue::Bool(false)))
        && get(&args, "show_in_rest").is_some_and(|v| !is_empty(v))
    {
        set(&mut args, "rest_namespace", s("wp/v2"));
    }
    // `$args['show_in_abilities'] = true === $args['show_in_abilities'];`
    let show_in_abilities = matches!(get(&args, "show_in_abilities"), Some(PhpValue::Bool(true)));
    set(
        &mut args,
        "show_in_abilities",
        PhpValue::Bool(show_in_abilities),
    );
    if is_null(&args, "show_in_menu") || is_empty(&arg(&args, "show_ui")) {
        let value = arg(&args, "show_ui");
        set(&mut args, "show_in_menu", value);
    }
    if is_null(&args, "show_in_admin_bar") {
        let shown = !is_empty(&arg(&args, "show_in_menu"));
        set(&mut args, "show_in_admin_bar", PhpValue::Bool(shown));
    }
    if is_null(&args, "show_in_nav_menus") {
        let value = arg(&args, "public");
        set(&mut args, "show_in_nav_menus", value);
    }
    if is_null(&args, "exclude_from_search") {
        let public = !is_empty(&arg(&args, "public"));
        set(&mut args, "exclude_from_search", PhpValue::Bool(!public));
    }

    // Back compat with 3.0's quirky handling (#14122), then the false default.
    let post_or_page = matches!(
        get(&args, "capability_type"),
        Some(PhpValue::Str(t)) if t.as_slice() == b"post" || t.as_slice() == b"page"
    );
    if is_empty(&arg(&args, "capabilities")) && is_null(&args, "map_meta_cap") && post_or_page {
        set(&mut args, "map_meta_cap", PhpValue::Bool(true));
    }
    if is_null(&args, "map_meta_cap") {
        set(&mut args, "map_meta_cap", PhpValue::Bool(false));
    }

    if is_empty(&arg(&args, "show_ui")) && !has_edit_link {
        set(&mut args, "_edit_link", s(""));
    }

    // Capabilities.
    let map_meta_cap = !is_empty(&arg(&args, "map_meta_cap"));
    let caps = post_type_capabilities(
        &arg(&args, "capability_type"),
        map_meta_cap,
        &arg(&args, "capabilities"),
    );
    let meta_caps = if map_meta_cap {
        meta_capabilities(&caps)
    } else {
        Vec::new()
    };
    set_property(
        &mut properties,
        PhpKey::Str(b"cap".to_vec()),
        PhpValue::Object {
            class: "stdClass".to_owned(),
            properties: caps,
        },
    );
    unset(&mut args, "capabilities");
    if let Some(PhpValue::Array(items)) = get(&args, "capability_type") {
        let first = items
            .iter()
            .find(|(key, _)| *key == PhpKey::Int(0))
            .map_or(PhpValue::Null, |(_, value)| value.clone());
        set(&mut args, "capability_type", first);
    }

    // query_var.
    match arg(&args, "query_var") {
        PhpValue::Bool(false) => {}
        PhpValue::Bool(true) => set(&mut args, "query_var", name_value.clone()),
        other => {
            let title = ports
                .titles
                .sanitize_title_with_dashes(&to_php_string(&other));
            set(&mut args, "query_var", PhpValue::Str(title));
        }
    }

    // Rewrite.
    let rewrite = arg(&args, "rewrite");
    if !matches!(rewrite, PhpValue::Bool(false)) && ports.rewrite.rewrite_applies()? {
        let mut rewrite = match rewrite {
            PhpValue::Array(items) => items,
            _ => Vec::new(),
        };
        if get(&rewrite, "slug").is_none_or(is_empty) {
            set(&mut rewrite, "slug", name_value.clone());
        }
        for key in ["with_front", "pages"] {
            if isset(&rewrite, key).is_none() {
                set(&mut rewrite, key, PhpValue::Bool(true));
            }
        }
        let has_archive = !is_empty(&arg(&args, "has_archive"));
        if isset(&rewrite, "feeds").is_none() || !has_archive {
            set(&mut rewrite, "feeds", PhpValue::Bool(has_archive));
        }
        if isset(&rewrite, "ep_mask").is_none() {
            // EP_PERMALINK.
            let ep_mask = isset(&args, "permalink_epmask")
                .cloned()
                .unwrap_or(PhpValue::Int(1));
            set(&mut rewrite, "ep_mask", ep_mask);
        }
        set(&mut args, "rewrite", PhpValue::Array(rewrite));
    }

    for (key, value) in args {
        set_property(&mut properties, key, value);
    }

    // Labels, from the object as it now stands.
    let label = get(&properties, "label").cloned();
    let labels_given = arg(&properties, "labels");
    let hierarchical = !is_empty(&arg(&properties, "hierarchical"));
    let source = LabelSource {
        name: name.as_str(),
        label: label.as_ref().filter(|v| !matches!(v, PhpValue::Null)),
        labels: &labels_given,
        hierarchical,
    };
    let labels = post_type_labels(&source, ports.translator)?;
    // With no callback, the filtered labels are the defaults themselves.
    let labels = merge_filtered_labels(labels.clone(), labels);

    let mut post_type = PostType::new(name, properties);
    store_labels(&mut post_type, labels);
    Ok(ResolvedPostType {
        post_type,
        meta_caps,
    })
}

/// `$this->labels = (object) $labels; $this->label = $this->labels->name;`.
fn store_labels(post_type: &mut PostType, labels: Vec<(PhpKey, PhpValue)>) {
    let label = get(&labels, "name").cloned().unwrap_or(PhpValue::Null);
    post_type.set(
        "labels",
        PhpValue::Object {
            class: "stdClass".to_owned(),
            properties: labels,
        },
    );
    post_type.set("label", label);
}

/// The stored labels as `(array)` reads them.
fn current_labels(post_type: &PostType) -> Vec<(PhpKey, PhpValue)> {
    match post_type.get("labels") {
        Some(PhpValue::Object { properties, .. }) => properties.clone(),
        Some(PhpValue::Array(items)) => items.clone(),
        Some(other) => to_array(other.clone()),
        None => Vec::new(),
    }
}

/// After the `post_type_labels_{$name}` filter: `labels = merge_filtered_labels(current, filtered)`;
/// `label = labels->name`.
pub fn apply_filtered_labels(post_type: &mut PostType, filtered: Vec<(PhpKey, PhpValue)>) {
    let current = current_labels(post_type);
    store_labels(post_type, merge_filtered_labels(current, filtered));
}

/// `get_post_type_labels()`'s filter (post.php 2228-2268, the filter at 2262) over a resolved post type:
/// `$labels = apply_filters( "post_type_labels_{$post_type}", $labels )` with the stored labels as a `stdClass`, then
/// `(object) array_merge( (array) $default_labels, (array) $labels )` through [`apply_filtered_labels`], and `label`
/// from the merged `name`.
///
/// With no callback on the hook nothing changes, since [`resolve_post_type`] already stored the unfiltered merge.
/// The filter sees a copy: a callback that keeps the object and changes it later changes nothing here.
pub fn filter_post_type_labels(
    hooks: &dyn HookApi,
    post_type: &mut PostType,
) -> Result<(), PostError> {
    let mut hook = b"post_type_labels_".to_vec();
    hook.extend_from_slice(post_type.name().as_str().as_bytes());
    let hook = byte_name(hook);
    if hooks.skip_if_empty(&hook, Count::Filters)? {
        return Ok(());
    }
    let labels = PhpValue::Object {
        class: "stdClass".to_owned(),
        properties: current_labels(post_type),
    };
    let filtered = pv(&hooks.apply_filters(&hook, hv(&labels), Vec::new())?)?;
    apply_filtered_labels(post_type, to_array(filtered));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::labels::Untranslated;

    struct Applies(bool);
    impl RewriteContext for Applies {
        fn rewrite_applies(&self) -> Result<bool, PostError> {
            Ok(self.0)
        }
    }

    struct Same;
    impl TitleSanitizer for Same {
        fn sanitize_title_with_dashes(&self, title: &[u8]) -> Vec<u8> {
            title.to_vec()
        }
    }

    fn k(v: &str) -> PhpKey {
        PhpKey::Str(v.as_bytes().to_vec())
    }

    fn resolve(applies: bool, args: Vec<(PhpKey, PhpValue)>) -> ResolvedPostType {
        let ports = PostTypePorts {
            translator: &Untranslated,
            rewrite: &Applies(applies),
            titles: &Same,
        };
        resolve_post_type(PostTypeName::new("book"), args, &ports).unwrap()
    }

    #[test]
    fn rewrite_stays_true_without_context() {
        let resolved = resolve(false, vec![(k("rewrite"), PhpValue::Bool(true))]);
        assert_eq!(
            resolved.post_type.get("rewrite"),
            Some(&PhpValue::Bool(true))
        );

        let resolved = resolve(true, vec![(k("rewrite"), PhpValue::Bool(true))]);
        assert_eq!(
            resolved.post_type.get("rewrite"),
            Some(&PhpValue::Array(vec![
                (k("slug"), s("book")),
                (k("with_front"), PhpValue::Bool(true)),
                (k("pages"), PhpValue::Bool(true)),
                (k("feeds"), PhpValue::Bool(false)),
                (k("ep_mask"), PhpValue::Int(1)),
            ]))
        );
    }

    #[test]
    fn apply_filtered_labels_replaces_label() {
        let mut post_type = resolve(false, Vec::new()).post_type;
        let before = match post_type.get("labels") {
            Some(PhpValue::Object { properties, .. }) => properties.clone(),
            _ => panic!("labels is an object"),
        };
        apply_filtered_labels(&mut post_type, vec![(k("name"), s("Tomes"))]);
        assert_eq!(post_type.get("label"), Some(&s("Tomes")));
        let Some(PhpValue::Object { class, properties }) = post_type.get("labels") else {
            panic!("labels is an object");
        };
        assert_eq!(class, "stdClass");
        assert_eq!(properties.len(), before.len());
        assert_eq!(get(properties, "name"), Some(&s("Tomes")));
        assert_eq!(get(properties, "add_new"), get(&before, "add_new"));
        assert_eq!(
            get(properties, "singular_name"),
            get(&before, "singular_name")
        );
    }
}
