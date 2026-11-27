//! The post type registry: `$wp_post_types`, `$_wp_post_type_features` and `$post_type_meta_caps` under
//! `register_post_type()`, `unregister_post_type()` (post.php, 7.2-RC2 lines 1859-1973), the post type supports
//! functions (2373-2442) and `WP_Post_Type`'s `add_supports()`, `register_meta_boxes()`, `add_hooks()`,
//! `register_taxonomies()` and their removal counterparts (class-wp-post-type.php 694-874).
//!
//! The registry hooks nothing but the labels filter ([`PostTypeRegistry::register_filtered`]) and registers no
//! taxonomy: it returns the actions and taxonomies for phase 4's adapter. Rewrite rules and query vars are left to
//! the rewrite seam. `add_supports()` walks an object `supports`' properties as it walks an array's entries, as
//! PHP's `foreach` does.
//!
//! The write side is pure, so phase 4's PHP adapter can apply it to `$_wp_post_type_features` and
//! `$post_type_meta_caps` itself: [`add_supports`] reads the features through the [`PostTypeSupports`] port and
//! returns the `add_post_type_support()` calls to make, and [`unregistration`] returns the features to remove and
//! the rebuilt meta capabilities. [`PostTypeRegistry`] is the in-memory adapter that applies both to its own maps.
//!
//! Approximations, none of them recorded:
//! - a non-empty `supports` that is neither an array nor an object adds no features (PHP warns in `foreach`) and is
//!   still removed;
//! - a `supports` list entry's value becomes a feature as PHP's `(array)` cast and array key conversion make it
//!   (`false` is `"0"`, `1.5` is `"1"`, null adds nothing);
//! - in the meta capability rebuild, a non-scalar value in a remaining type's `cap` is skipped (PHP throws "Illegal
//!   offset type"), and a `cap` that is an array rather than an object is read as the object would be (PHP's
//!   `get_object_vars()` throws a `TypeError`).

use indexmap::IndexMap;
use speedyspec_php_serialize::semantics::{is_empty, key_to_string, to_array, to_array_key};
use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_hooks_contracts::HookApi;
use speedyspec_wp_posts_contracts::{
    PostError, PostType, PostTypeError, PostTypeName, PostTypeSupports, PostTypes,
};

use crate::caps::meta_capabilities;
use crate::post_type::{
    PostTypePorts, ResolvedPostType, filter_post_type_labels, resolve_post_type,
};

/// An `add_action()` call the adapter makes.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionToAdd {
    /// The hook name.
    pub hook: String,
    /// The callback, as registered.
    pub callback: PhpValue,
    /// The priority.
    pub priority: i64,
    /// The accepted args.
    pub accepted_args: u32,
}

/// A `remove_action()` call the adapter makes.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionToRemove {
    /// The hook name.
    pub hook: String,
    /// The callback, as registered.
    pub callback: PhpValue,
    /// The priority.
    pub priority: i64,
}

/// What `register_post_type()` stored and what it leaves to the adapter.
#[derive(Debug, Clone)]
pub struct PostTypeRegistration {
    /// The object as stored, after `add_supports()`.
    pub post_type: PostType,
    /// `register_meta_boxes()`: `("add_meta_boxes_{name}", cb, 10, 1)` when the callback is truthy; then
    /// `add_hooks()`: `("future_{name}", "_future_post_hook", 5, 2)`.
    pub actions: Vec<ActionToAdd>,
    /// `register_taxonomies()`: each value of an array `taxonomies`, for `register_taxonomy_for_object_type()`
    /// (phase 4).
    pub taxonomies: Vec<PhpValue>,
}

/// What `unregister_post_type()` removes and what it leaves to the adapter.
#[derive(Debug, Clone)]
pub struct PostTypeUnregistration {
    /// The object that was registered.
    pub post_type: PostType,
    /// `remove_supports()`: the `$_wp_post_type_features` key to unset (the post type's name).
    pub supports_to_remove: String,
    /// `$post_type_meta_caps` after the type is unset: emptied, then rebuilt by `_post_type_meta_capabilities()`
    /// from the `cap` of every remaining type whose `map_meta_cap` is truthy, in registration order, `(custom,
    /// core)` in insertion order.
    pub meta_caps: Vec<(String, &'static str)>,
    /// `unregister_meta_boxes()` (priority 10, when the callback is truthy), then `remove_hooks()` (priority 5).
    pub actions: Vec<ActionToRemove>,
}

/// An `add_post_type_support( $post_type, $feature, ...$args )` call the adapter makes.
#[derive(Debug, Clone, PartialEq)]
pub struct SupportToAdd {
    /// The post type.
    pub post_type: String,
    /// `$feature`, as the list `(array) $feature` makes it. Never empty: a call with no features adds nothing and
    /// is left out.
    pub features: Vec<String>,
    /// `...$args`: empty stores `true`, otherwise the list is stored.
    pub args: Vec<PhpValue>,
}

impl SupportToAdd {
    fn new(post_type: &str, features: Vec<String>, args: Vec<PhpValue>) -> Self {
        Self {
            post_type: post_type.to_owned(),
            features,
            args,
        }
    }

    fn adds(&self, feature: &str) -> bool {
        self.features.iter().any(|f| f == feature)
    }
}

/// `WP_Post_Type::add_supports()`: the `add_post_type_support()` calls it makes, in order, given the features
/// `supports` holds before them. Unsets `supports` on the object when it was not empty, as WordPress does.
///
/// The `editor`-implies-`autosave` rule asks `post_type_supports()` after the loop's calls, so it counts a feature
/// that was already there (a plugin's `add_post_type_support()` before registration) as well as one the loop adds.
/// `add_post_type_support()` only adds, so that is `supports` or the calls.
pub fn add_supports(
    post_type: &mut PostType,
    supports: &dyn PostTypeSupports,
) -> Vec<SupportToAdd> {
    let name = post_type.name().as_str().to_owned();
    let value = post_type.get("supports").cloned().unwrap_or(PhpValue::Null);
    let mut calls = Vec::new();
    if !is_empty(&value) {
        // `foreach ( $this->supports as $feature => $args )`: an array's entries or an object's properties.
        if let PhpValue::Array(items)
        | PhpValue::Object {
            properties: items, ..
        } = value
        {
            for (feature, args) in items {
                if matches!(args, PhpValue::Array(_)) {
                    calls.push(SupportToAdd::new(
                        &name,
                        vec![key_to_string(&feature)],
                        vec![args],
                    ));
                } else {
                    let features: Vec<String> = to_array(args)
                        .iter()
                        .filter_map(|(_, f)| to_array_key(f))
                        .map(|f| key_to_string(&f))
                        .collect();
                    if !features.is_empty() {
                        calls.push(SupportToAdd::new(&name, features, Vec::new()));
                    }
                }
            }
        }
        post_type.remove("supports");
        // 'editor' implies 'autosave' for backward compatibility.
        let supported = |feature: &str| {
            supports.supports(&name, feature) || calls.iter().any(|c| c.adds(feature))
        };
        if supported("editor") && !supported("autosave") {
            calls.push(SupportToAdd::new(
                &name,
                vec!["autosave".to_owned()],
                Vec::new(),
            ));
        }
    } else if !matches!(value, PhpValue::Bool(false)) {
        let defaults = ["title", "editor", "autosave"].map(str::to_owned).to_vec();
        calls.push(SupportToAdd::new(&name, defaults, Vec::new()));
    }
    calls
}

/// `unregister_post_type()` for a registered type: `Builtin` when its `_builtin` is truthy; otherwise its features
/// and actions to remove, and `$post_type_meta_caps` rebuilt from `registered`, the post types registered before
/// the call (the type itself among them, which the rebuild skips).
///
/// WordPress 7.2 rebuilds the meta capabilities instead of unsetting the type's own: one custom capability may be
/// owed to several types, so removing this type's entries could remove ones the others still need.
pub fn unregistration(
    post_type: PostType,
    registered: &dyn PostTypes,
) -> Result<PostTypeUnregistration, PostTypeError> {
    if post_type.get("_builtin").is_some_and(|b| !is_empty(b)) {
        return Err(PostTypeError::Builtin);
    }
    let name = post_type.name().as_str().to_owned();

    // `$post_type_meta_caps = array();` then, for each remaining type with a truthy `map_meta_cap`,
    // `_post_type_meta_capabilities( get_object_vars( $registered_post_type->cap ) )`.
    let mut meta_caps: IndexMap<String, &'static str> = IndexMap::new();
    for remaining in registered.post_types() {
        if remaining.name().as_str() == name || remaining.get("map_meta_cap").is_none_or(is_empty) {
            continue;
        }
        let caps = match remaining.get("cap") {
            Some(PhpValue::Object { properties, .. }) => properties.as_slice(),
            Some(PhpValue::Array(items)) => items.as_slice(),
            _ => &[],
        };
        for (custom, core) in meta_capabilities(caps) {
            meta_caps.insert(custom, core);
        }
    }

    let mut actions = Vec::new();
    if let Some(cb) = post_type
        .get("register_meta_box_cb")
        .filter(|cb| !is_empty(cb))
    {
        actions.push(ActionToRemove {
            hook: format!("add_meta_boxes_{name}"),
            callback: cb.clone(),
            priority: 10,
        });
    }
    actions.push(ActionToRemove {
        hook: format!("future_{name}"),
        callback: PhpValue::Str(FUTURE_POST_HOOK.as_bytes().to_vec()),
        priority: 5,
    });
    Ok(PostTypeUnregistration {
        post_type,
        supports_to_remove: name,
        meta_caps: meta_caps.into_iter().collect(),
        actions,
    })
}

/// `$wp_post_types`, `$_wp_post_type_features` and `$post_type_meta_caps`, in insertion order.
#[derive(Clone, Debug, Default)]
pub struct PostTypeRegistry {
    post_types: IndexMap<String, PostType>,
    features: IndexMap<String, IndexMap<String, PhpValue>>,
    meta_caps: IndexMap<String, &'static str>,
}

const FUTURE_POST_HOOK: &str = "_future_post_hook";

impl PostTypeRegistry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// `register_post_type()` after `check_post_type_name()` and set_props()' args filters: resolves the object
    /// (remembering its meta capabilities), runs `add_supports()`, stores it (in place when the name is already
    /// registered), and returns its actions and taxonomies.
    pub fn register(
        &mut self,
        name: PostTypeName,
        args: Vec<(PhpKey, PhpValue)>,
        ports: &PostTypePorts<'_>,
    ) -> PostTypeRegistration {
        self.store(resolve_post_type(name, args, ports))
    }

    /// [`Self::register`] with the `post_type_labels_{$name}` filter run on the new object
    /// ([`filter_post_type_labels`]), where `set_props()` runs it: before `add_supports()` and before the object is
    /// stored. A callback's failure leaves the registry unchanged.
    pub fn register_filtered(
        &mut self,
        name: PostTypeName,
        args: Vec<(PhpKey, PhpValue)>,
        ports: &PostTypePorts<'_>,
        hooks: &dyn HookApi,
    ) -> Result<PostTypeRegistration, PostError> {
        let mut resolved = resolve_post_type(name, args, ports);
        filter_post_type_labels(hooks, &mut resolved.post_type)?;
        Ok(self.store(resolved))
    }

    /// The rest of `register_post_type()` once the object is built.
    fn store(&mut self, resolved: ResolvedPostType) -> PostTypeRegistration {
        for (custom, core) in resolved.meta_caps {
            self.meta_caps.insert(custom, core);
        }
        let mut post_type = resolved.post_type;
        for call in add_supports(&mut post_type, &*self) {
            self.apply_support(call);
        }

        let name = post_type.name().as_str().to_owned();
        let mut actions = Vec::new();
        if let Some(cb) = post_type
            .get("register_meta_box_cb")
            .filter(|cb| !is_empty(cb))
        {
            actions.push(ActionToAdd {
                hook: format!("add_meta_boxes_{name}"),
                callback: cb.clone(),
                priority: 10,
                accepted_args: 1,
            });
        }
        self.post_types.insert(name.clone(), post_type.clone());
        actions.push(ActionToAdd {
            hook: format!("future_{name}"),
            callback: PhpValue::Str(FUTURE_POST_HOOK.as_bytes().to_vec()),
            priority: 5,
            accepted_args: 2,
        });

        // `foreach ( $this->taxonomies as $taxonomy )`: an array's values (an object's properties); nothing else.
        let taxonomies = match post_type.get("taxonomies") {
            Some(PhpValue::Array(items))
            | Some(PhpValue::Object {
                properties: items, ..
            }) => items.iter().map(|(_, v)| v.clone()).collect(),
            _ => Vec::new(),
        };

        PostTypeRegistration {
            post_type,
            actions,
            taxonomies,
        }
    }

    /// `unregister_post_type()`: `Invalid` when the type is missing, `Builtin` when its `_builtin` is truthy.
    /// Otherwise applies [`unregistration`]: removes the type's features and the entry, and replaces the meta
    /// capabilities with the rebuilt ones.
    pub fn unregister(&mut self, name: &str) -> Result<PostTypeUnregistration, PostTypeError> {
        let post_type = self.post_types.get(name).ok_or(PostTypeError::Invalid)?;
        let unregistration = unregistration(post_type.clone(), &*self)?;
        self.features
            .shift_remove(&unregistration.supports_to_remove);
        self.post_types.shift_remove(name);
        self.meta_caps = unregistration.meta_caps.iter().cloned().collect();
        Ok(unregistration)
    }

    /// Makes one [`SupportToAdd`] call.
    pub fn apply_support(&mut self, call: SupportToAdd) {
        let features: Vec<&str> = call.features.iter().map(String::as_str).collect();
        self.add_support(&call.post_type, &features, call.args);
    }

    /// `add_post_type_support()`: each feature set to `true`, or to the `args` list when it is non-empty. No
    /// features leaves the registry as it was.
    pub fn add_support(&mut self, post_type: &str, features: &[&str], args: Vec<PhpValue>) {
        if features.is_empty() {
            return;
        }
        let value = if args.is_empty() {
            PhpValue::Bool(true)
        } else {
            PhpValue::Array(
                args.into_iter()
                    .enumerate()
                    .map(|(i, v)| (PhpKey::Int(i as i64), v))
                    .collect(),
            )
        };
        let entry = self.features.entry(post_type.to_owned()).or_default();
        for feature in features {
            entry.insert((*feature).to_owned(), value.clone());
        }
    }

    /// `remove_post_type_support()`: the type's entry stays, empty, after its last feature goes.
    pub fn remove_support(&mut self, post_type: &str, feature: &str) {
        if let Some(features) = self.features.get_mut(post_type) {
            features.shift_remove(feature);
        }
    }

    /// `get_all_post_type_supports()`.
    pub fn all_supports(&self, post_type: &str) -> Vec<(String, PhpValue)> {
        self.features
            .get(post_type)
            .map_or_else(Vec::new, |features| {
                features
                    .iter()
                    .map(|(f, v)| (f.clone(), v.clone()))
                    .collect()
            })
    }

    /// `$post_type_meta_caps[ $cap ]`.
    pub fn meta_cap(&self, cap: &str) -> Option<&'static str> {
        self.meta_caps.get(cap).copied()
    }

    /// `$post_type_meta_caps`, `(custom, core)` in insertion order.
    pub fn meta_caps(&self) -> Vec<(String, &'static str)> {
        self.meta_caps
            .iter()
            .map(|(custom, core)| (custom.clone(), *core))
            .collect()
    }
}

impl PostTypes for PostTypeRegistry {
    fn post_type(&self, name: &str) -> Option<PostType> {
        self.post_types.get(name).cloned()
    }

    fn post_types(&self) -> Vec<PostType> {
        self.post_types.values().cloned().collect()
    }
}

impl PostTypeSupports for PostTypeRegistry {
    /// `post_type_supports()`: `isset()`, and a stored feature is never null.
    fn supports(&self, post_type: &str, feature: &str) -> bool {
        self.features
            .get(post_type)
            .is_some_and(|features| features.contains_key(feature))
    }

    fn features(&self) -> Vec<(String, Vec<(String, PhpValue)>)> {
        self.features
            .keys()
            .map(|post_type| (post_type.clone(), self.all_supports(post_type)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> PhpValue {
        PhpValue::Str(v.as_bytes().to_vec())
    }

    fn k(v: &str) -> PhpKey {
        PhpKey::Str(v.as_bytes().to_vec())
    }

    fn list(items: &[&str]) -> PhpValue {
        PhpValue::Array(
            items
                .iter()
                .zip(0..)
                .map(|(v, i)| (PhpKey::Int(i), s(v)))
                .collect(),
        )
    }

    /// `$_wp_post_type_features` as a plugin left it before `register_post_type()`.
    struct Features(Vec<(String, Vec<(String, PhpValue)>)>);

    impl PostTypeSupports for Features {
        fn supports(&self, post_type: &str, feature: &str) -> bool {
            self.0
                .iter()
                .any(|(t, fs)| t == post_type && fs.iter().any(|(f, _)| f == feature))
        }

        fn features(&self) -> Vec<(String, Vec<(String, PhpValue)>)> {
            self.0.clone()
        }
    }

    fn book(supports: PhpValue) -> PostType {
        PostType::new(
            PostTypeName::new("book"),
            vec![(k("name"), s("book")), (k("supports"), supports)],
        )
    }

    fn call(features: &[&str], args: Vec<PhpValue>) -> SupportToAdd {
        SupportToAdd::new(
            "book",
            features.iter().map(|f| (*f).to_owned()).collect(),
            args,
        )
    }

    #[test]
    fn editor_added_by_a_plugin_before_registration_implies_autosave() {
        // add_post_type_support( 'book', 'editor' ); register_post_type( 'book', array( 'supports' => array( 'title' ) ) ):
        // post_type_supports( 'book', 'editor' ) is true after the loop, so WordPress adds 'autosave'.
        let plugin = Features(vec![(
            "book".to_owned(),
            vec![("editor".to_owned(), PhpValue::Bool(true))],
        )]);
        let mut post_type = book(list(&["title"]));
        assert_eq!(
            add_supports(&mut post_type, &plugin),
            [
                call(&["title"], Vec::new()),
                call(&["autosave"], Vec::new())
            ]
        );
        assert_eq!(post_type.get("supports"), None);

        // With 'autosave' there too, nothing more is added.
        let plugin = Features(vec![(
            "book".to_owned(),
            vec![
                ("editor".to_owned(), PhpValue::Bool(true)),
                ("autosave".to_owned(), PhpValue::Bool(true)),
            ],
        )]);
        let mut post_type = book(list(&["title"]));
        assert_eq!(
            add_supports(&mut post_type, &plugin),
            [call(&["title"], Vec::new())]
        );
    }

    #[test]
    fn add_supports_keeps_wordpress_order() {
        let none = Features(Vec::new());
        let supports = PhpValue::Array(vec![
            (PhpKey::Int(0), s("editor")),
            (k("thumbnail"), list(&["x"])),
            (PhpKey::Int(1), PhpValue::Null),
        ]);
        let mut post_type = book(supports);
        assert_eq!(
            add_supports(&mut post_type, &none),
            [
                call(&["editor"], Vec::new()),
                call(&["thumbnail"], vec![list(&["x"])]),
                call(&["autosave"], Vec::new()),
            ]
        );

        // Empty `supports` adds the defaults and stays; `false` adds nothing.
        let mut post_type = book(PhpValue::Null);
        assert_eq!(
            add_supports(&mut post_type, &none),
            [call(&["title", "editor", "autosave"], Vec::new())]
        );
        assert_eq!(post_type.get("supports"), Some(&PhpValue::Null));
        let mut post_type = book(PhpValue::Bool(false));
        assert!(add_supports(&mut post_type, &none).is_empty());
    }

    #[test]
    fn unregistration_returns_what_to_remove() {
        let cap = |edit: PhpValue| PhpValue::Object {
            class: "stdClass".into(),
            properties: vec![
                (k("edit_post"), edit),
                (k("read_post"), PhpValue::Array(Vec::new())),
                (k("read"), PhpValue::Int(7)),
            ],
        };
        let typed = |name: &str, edit: &str, map_meta_cap: bool| {
            PostType::new(
                PostTypeName::new(name),
                vec![
                    (k("name"), s(name)),
                    (k("map_meta_cap"), PhpValue::Bool(map_meta_cap)),
                    (k("cap"), cap(s(edit))),
                ],
            )
        };
        let mut registry = PostTypeRegistry::new();
        for post_type in [
            typed("book", "edit_tome", true),
            typed("scroll", "edit_tome", true),
            typed("note", "edit_note", false),
        ] {
            registry
                .post_types
                .insert(post_type.name().as_str().to_owned(), post_type);
        }
        let removal = unregistration(registry.post_types["book"].clone(), &registry).unwrap();
        assert_eq!(removal.supports_to_remove, "book");
        // Rebuilt from the types that remain with a truthy map_meta_cap: `scroll` still owes `edit_tome`, `note`
        // adds nothing, and a non-key `cap` value is skipped (PHP throws "Illegal offset type").
        assert_eq!(removal.meta_caps, [("edit_tome".to_owned(), "edit_post")]);
        assert_eq!(removal.actions.len(), 1);

        let builtin = PostType::new(
            PostTypeName::new("post"),
            vec![(k("_builtin"), PhpValue::Bool(true))],
        );
        assert_eq!(
            unregistration(builtin, &registry).unwrap_err(),
            PostTypeError::Builtin
        );
    }
}
