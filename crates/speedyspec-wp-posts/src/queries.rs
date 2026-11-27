//! The post type and status predicates: `post_type_exists()` (post.php, 7.2-RC2 lines 1610-1612),
//! `is_post_type_hierarchical()` (1587-1594), `is_post_type_viewable()` (2496-2526) and `is_post_status_viewable()`
//! (2540-2578).
//!
//! [`is_post_type_viewable`] and [`is_post_status_viewable`] are the values passed to the `is_post_type_viewable` and
//! `is_post_status_viewable` filters; the `*_filtered` functions run those filters (Observer, through [`HookApi`])
//! with WordPress's `true ===` check, so only a filter answer of exactly `true` is viewable. The adapter answers the
//! `is_scalar()` and `is_string()` checks on a name.

use speedyspec_php_serialize::semantics::{get, is_empty};
use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_hooks_contracts::{HookApi, HookName, HookValue};
use speedyspec_wp_posts_contracts::{
    PostError, PostStatusObject, PostStatuses, PostType, PostTypes,
};

use crate::hook::{filter_hook, hv};

/// PHP truth of a property; a missing one reads as null.
fn truthy(value: Option<&PhpValue>) -> bool {
    value.is_some_and(|v| !is_empty(v))
}

/// `publicly_queryable || ( _builtin && public )`, from an object's properties.
fn viewable(properties: &[(PhpKey, PhpValue)]) -> bool {
    let prop = |name| truthy(get(properties, name));
    prop("publicly_queryable") || (prop("_builtin") && prop("public"))
}

/// `post_type_exists()`: a stored post type is an object, never `empty()`.
pub fn post_type_exists(types: &dyn PostTypes, name: &str) -> bool {
    types.post_type(name).is_some()
}

/// `is_post_type_hierarchical()`: false when missing, else the truth of `hierarchical`. WordPress returns the
/// property as it is; a caller that needs the raw value reads it from the post type.
pub fn is_post_type_hierarchical(types: &dyn PostTypes, name: &str) -> bool {
    types
        .post_type(name)
        .is_some_and(|t| truthy(t.get("hierarchical")))
}

/// `is_post_type_viewable()` for an object: `publicly_queryable || ( _builtin && public )`, by PHP truth.
pub fn is_post_type_viewable(post_type: &PostType) -> bool {
    viewable(post_type.properties())
}

/// `is_post_status_viewable()` for an object: false when `internal` or `protected`; else
/// `publicly_queryable || ( _builtin && public )`, by PHP truth.
pub fn is_post_status_viewable(status: &PostStatusObject) -> bool {
    if truthy(status.get("internal")) || truthy(status.get("protected")) {
        return false;
    }
    viewable(status.properties())
}

/// `is_post_type_viewable()` for a name: false when the post type is missing.
pub fn post_type_name_viewable(types: &dyn PostTypes, name: &str) -> bool {
    types
        .post_type(name)
        .is_some_and(|t| is_post_type_viewable(&t))
}

/// `is_post_status_viewable()` for a string name: false when the status is missing.
pub fn post_status_name_viewable(statuses: &dyn PostStatuses, name: &str) -> bool {
    statuses
        .status(name)
        .is_some_and(|s| is_post_status_viewable(&s))
}

/// `true === apply_filters( $hook, $is_viewable, $object )`.
fn filtered_viewable(
    hooks: &dyn HookApi,
    hook: &str,
    is_viewable: bool,
    object: impl FnOnce() -> PhpValue,
) -> Result<bool, PostError> {
    let out = filter_hook(
        hooks,
        &HookName::from(hook),
        HookValue::Bool(is_viewable),
        || vec![hv(&object())],
    )?;
    Ok(matches!(&*out.read(), HookValue::Bool(true)))
}

/// `is_post_type_viewable( $post_type )` for an object, with its filter: [`is_post_type_viewable`] through
/// `is_post_type_viewable` with `( $is_viewable, $post_type )`, true only when the filter answers exactly `true`.
/// The filter sees a `WP_Post_Type`-classed copy of the object.
pub fn is_post_type_viewable_filtered(
    hooks: &dyn HookApi,
    post_type: &PostType,
) -> Result<bool, PostError> {
    filtered_viewable(
        hooks,
        "is_post_type_viewable",
        is_post_type_viewable(post_type),
        || PhpValue::Object {
            class: "WP_Post_Type".to_owned(),
            properties: post_type.properties().to_vec(),
        },
    )
}

/// `is_post_status_viewable( $post_status )` for an object, with its filter: false for an `internal` or
/// `protected` status before any filter; else [`is_post_status_viewable`] through `is_post_status_viewable` with
/// `( $is_viewable, $post_status )`, true only when the filter answers exactly `true`. The filter sees a `stdClass`
/// copy of the object, as `register_post_status()` stores one.
pub fn is_post_status_viewable_filtered(
    hooks: &dyn HookApi,
    status: &PostStatusObject,
) -> Result<bool, PostError> {
    if truthy(status.get("internal")) || truthy(status.get("protected")) {
        return Ok(false);
    }
    filtered_viewable(
        hooks,
        "is_post_status_viewable",
        viewable(status.properties()),
        || PhpValue::Object {
            class: "stdClass".to_owned(),
            properties: status.properties().to_vec(),
        },
    )
}

/// `is_post_type_viewable()` for a name, with its filter: false, unfiltered, when the post type is missing.
pub fn post_type_name_viewable_filtered(
    hooks: &dyn HookApi,
    types: &dyn PostTypes,
    name: &str,
) -> Result<bool, PostError> {
    match types.post_type(name) {
        Some(post_type) => is_post_type_viewable_filtered(hooks, &post_type),
        None => Ok(false),
    }
}

/// `is_post_status_viewable()` for a string name, with its filter: false, unfiltered, when the status is missing.
pub fn post_status_name_viewable_filtered(
    hooks: &dyn HookApi,
    statuses: &dyn PostStatuses,
    name: &str,
) -> Result<bool, PostError> {
    match statuses.status(name) {
        Some(status) => is_post_status_viewable_filtered(hooks, &status),
        None => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use speedyspec_wp_posts_contracts::{PostStatusName, PostTypeName};

    fn props(values: &[(&str, PhpValue)]) -> Vec<(PhpKey, PhpValue)> {
        values
            .iter()
            .map(|(p, v)| (PhpKey::Str(p.as_bytes().to_vec()), v.clone()))
            .collect()
    }

    struct Types(Vec<PostType>);

    impl PostTypes for Types {
        fn post_type(&self, name: &str) -> Option<PostType> {
            self.0.iter().find(|t| t.name().as_str() == name).cloned()
        }

        fn post_types(&self) -> Vec<PostType> {
            self.0.clone()
        }
    }

    struct Statuses(Vec<PostStatusObject>);

    impl PostStatuses for Statuses {
        fn status(&self, name: &str) -> Option<PostStatusObject> {
            self.0.iter().find(|s| s.name().as_str() == name).cloned()
        }

        fn statuses(&self) -> Vec<PostStatusObject> {
            self.0.clone()
        }
    }

    #[test]
    fn missing_names_are_not_viewable() {
        let types = Types(Vec::new());
        let statuses = Statuses(Vec::new());
        assert!(!post_type_exists(&types, "missing"));
        assert!(!is_post_type_hierarchical(&types, "missing"));
        assert!(!post_type_name_viewable(&types, "missing"));
        assert!(!post_status_name_viewable(&statuses, "missing"));
    }

    #[test]
    fn builtin_public_type_is_viewable_without_publicly_queryable() {
        let builtin = |public: PhpValue| {
            PostType::new(
                PostTypeName::new("page"),
                props(&[
                    ("public", public),
                    ("publicly_queryable", PhpValue::Bool(false)),
                    ("_builtin", PhpValue::Bool(true)),
                    ("hierarchical", PhpValue::Int(1)),
                ]),
            )
        };
        assert!(is_post_type_viewable(&builtin(PhpValue::Bool(true))));
        assert!(!is_post_type_viewable(&builtin(PhpValue::Bool(false))));
        let types = Types(vec![builtin(PhpValue::Str(b"yes".to_vec()))]);
        assert!(post_type_exists(&types, "page"));
        assert!(is_post_type_hierarchical(&types, "page"));
        assert!(post_type_name_viewable(&types, "page"));
    }

    #[test]
    fn protected_status_is_never_viewable() {
        let status = |protected: PhpValue, internal: PhpValue| {
            PostStatusObject::new(
                PostStatusName::new("held"),
                props(&[
                    ("public", PhpValue::Bool(true)),
                    ("publicly_queryable", PhpValue::Bool(true)),
                    ("_builtin", PhpValue::Bool(true)),
                    ("protected", protected),
                    ("internal", internal),
                ]),
            )
        };
        assert!(!is_post_status_viewable(&status(
            PhpValue::Bool(true),
            PhpValue::Bool(false)
        )));
        assert!(!is_post_status_viewable(&status(
            PhpValue::Bool(false),
            PhpValue::Int(1)
        )));
        assert!(is_post_status_viewable(&status(
            PhpValue::Bool(false),
            PhpValue::Bool(false)
        )));
        let statuses = Statuses(vec![status(PhpValue::Bool(true), PhpValue::Null)]);
        assert!(!post_status_name_viewable(&statuses, "held"));
    }
}
