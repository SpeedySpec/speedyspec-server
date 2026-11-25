//! The post type registry: `$wp_post_types`, `$_wp_post_type_features` and `$post_type_meta_caps` under
//! `register_post_type()`, `unregister_post_type()` (post.php, 7.1.5 lines 1857-1962), the post type supports
//! functions (2326-2395) and `WP_Post_Type`'s `add_supports()`, `register_meta_boxes()`, `add_hooks()`,
//! `register_taxonomies()` and their removal counterparts (class-wp-post-type.php 676-860).
//!
//! The registry hooks nothing and registers no taxonomy: it returns the actions and taxonomies for phase 4's
//! adapter. Rewrite rules and query vars are left to the rewrite seam.
//!
//! Approximations, none of them recorded:
//! - a non-empty, non-array `supports` adds no features (PHP warns in `foreach`) and is still removed;
//! - a `supports` list entry's value becomes a feature as PHP's `(array)` cast and array key conversion make it
//!   (`false` is `"0"`, `1.5` is `"1"`, null adds nothing);
//! - a non-scalar value in `cap` removes no meta capability (PHP throws "Illegal offset type").

use indexmap::IndexMap;
use speedyspec_php_serialize::semantics::{is_empty, key_to_string, to_array, to_array_key};
use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_posts_contracts::{
    PostType, PostTypeError, PostTypeName, PostTypeSupports, PostTypes,
};

use crate::post_type::{PostTypePorts, resolve_post_type};

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

/// What `unregister_post_type()` removed and what it leaves to the adapter.
#[derive(Debug, Clone)]
pub struct PostTypeUnregistration {
    /// The object that was registered.
    pub post_type: PostType,
    /// `unregister_meta_boxes()` (priority 10, when the callback is truthy), then `remove_hooks()` (priority 5).
    pub actions: Vec<ActionToRemove>,
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
        let resolved = resolve_post_type(name, args, ports);
        for (custom, core) in resolved.meta_caps {
            self.meta_caps.insert(custom, core);
        }
        let mut post_type = resolved.post_type;
        self.add_supports(&mut post_type);

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

    /// `WP_Post_Type::add_supports()`.
    fn add_supports(&mut self, post_type: &mut PostType) {
        let name = post_type.name().as_str().to_owned();
        let supports = post_type.get("supports").cloned().unwrap_or(PhpValue::Null);
        if !is_empty(&supports) {
            if let PhpValue::Array(items) = supports {
                for (feature, args) in items {
                    if matches!(args, PhpValue::Array(_)) {
                        self.add_support(&name, &[&key_to_string(&feature)], vec![args]);
                    } else {
                        let features: Vec<String> = to_array(args)
                            .iter()
                            .filter_map(|(_, f)| to_array_key(f))
                            .map(|f| key_to_string(&f))
                            .collect();
                        let features: Vec<&str> = features.iter().map(String::as_str).collect();
                        self.add_support(&name, &features, Vec::new());
                    }
                }
            }
            post_type.remove("supports");
            // 'editor' implies 'autosave' for backward compatibility.
            if self.supports(&name, "editor") && !self.supports(&name, "autosave") {
                self.add_support(&name, &["autosave"], Vec::new());
            }
        } else if !matches!(supports, PhpValue::Bool(false)) {
            self.add_support(&name, &["title", "editor", "autosave"], Vec::new());
        }
    }

    /// `unregister_post_type()`: `Invalid` when the type is missing, `Builtin` when its `_builtin` is truthy.
    /// Otherwise removes the type's features, the meta capability of every `cap` value and the entry.
    pub fn unregister(&mut self, name: &str) -> Result<PostTypeUnregistration, PostTypeError> {
        let post_type = self.post_types.get(name).ok_or(PostTypeError::Invalid)?;
        if post_type.get("_builtin").is_some_and(|b| !is_empty(b)) {
            return Err(PostTypeError::Builtin);
        }

        // remove_supports().
        self.features.shift_remove(name);

        // remove_rewrite_rules()' meta capabilities: `unset( $post_type_meta_caps[ $cap ] )` for every cap.
        let caps = match post_type.get("cap") {
            Some(PhpValue::Object { properties, .. }) => properties.clone(),
            Some(PhpValue::Array(items)) => items.clone(),
            _ => Vec::new(),
        };
        for (_, cap) in caps {
            if let Some(cap) = to_array_key(&cap) {
                self.meta_caps.shift_remove(&key_to_string(&cap));
            }
        }

        let post_type = self
            .post_types
            .shift_remove(name)
            .expect("checked present above");
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
        Ok(PostTypeUnregistration { post_type, actions })
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
