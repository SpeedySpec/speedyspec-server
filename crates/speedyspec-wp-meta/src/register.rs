//! `register_meta()`'s rules, ported from WordPress 7.1.5's `wp-includes/meta.php`.
//!
//! The domain decides; the phase-3 adapter applies. It calls [`prepare_registration`] (the steps before the
//! `register_meta_args` filter), runs that filter on [`PreparedArgs::args`], then calls [`resolve_registration`]
//! (everything after it). It adds the [`FilterToAdd`]s, adds `filter_default_metadata` when asked and stores a
//! [`Registration::Registered`] key in the [`MetaKeyRegistry`](crate::MetaKeyRegistry).
//!
//! Approximations, none of which WordPress records (it documents the subtype as a string):
//! - a float `object_subtype` is truncated to an int, as the array key is, though PHP's hook name keeps the decimals;
//! - an array or object `object_subtype` becomes `""`, where PHP throws;
//! - a non-array `show_in_rest.schema` skips the merge into the default's schema, where PHP's `array_merge` throws.

use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_meta_contracts::{Callables, DefaultSchemaCheck, MetaKey, RevisionSupport};

use crate::hook_names::MetaHookNames;
use crate::php::{array_merge, get, is_empty, isset, set, unset};
use crate::protected::is_protected_key;

/// `register_meta()`'s `$args` after the steps before the `register_meta_args` filter.
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedArgs {
    /// What the `register_meta_args` filter receives; the adapter may replace it with the filter's answer.
    pub args: Vec<(PhpKey, PhpValue)>,
    /// `$args` was itself a callable: the pre-4.6.0 sanitize form.
    pub has_old_sanitize_cb: bool,
    /// `$deprecated` was a callable: the pre-4.6.0 auth form.
    pub has_old_auth_cb: bool,
}

/// One `add_filter()` call `register_meta()` makes.
#[derive(Clone, Debug, PartialEq)]
pub struct FilterToAdd {
    pub hook: Vec<u8>,
    pub callback: PhpValue,
    pub priority: i64,
    pub accepted_args: u32,
}

/// What `register_meta()` does.
#[derive(Clone, Debug, PartialEq)]
pub enum Registration {
    /// Stored in the registry; register_meta() returns true.
    Registered {
        key: RegisteredMetaKey,
        filters: Vec<FilterToAdd>,
        /// Add `filter_default_metadata` to `default_{type}_metadata` (10, 5) unless it is already there.
        add_default_filter: bool,
    },
    /// Legacy forms: filters added, nothing stored; register_meta() returns false.
    HooksOnly {
        filters: Vec<FilterToAdd>,
        add_default_filter: bool,
    },
    /// _doing_it_wrong(function, message, version); returns false.
    ///
    /// `filters` are the ones WordPress had already added before it gave up: the default-type check comes after
    /// the sanitize and auth filters are added, so they stay (WordPress 7.1.5 records this).
    Rejected {
        message: String,
        version: &'static str,
        filters: Vec<FilterToAdd>,
    },
}

/// One entry of `$wp_meta_keys[ $object_type ][ $subtype ][ $key ]`: the parsed args, without `object_subtype`.
#[derive(Clone, Debug, PartialEq)]
pub struct RegisteredMetaKey {
    pub object_type: String,
    pub subtype: String,
    pub key: MetaKey,
    pub args: Vec<(PhpKey, PhpValue)>,
}

impl RegisteredMetaKey {
    /// `$args['single']`, as PHP tests it for truth.
    pub fn single(&self) -> bool {
        get(&self.args, "single").is_some_and(|v| !is_empty(v))
    }

    /// `$args['default']` when the key was registered with one (`array_key_exists`, so a null default counts).
    pub fn default(&self) -> Option<&PhpValue> {
        get(&self.args, "default")
    }

    /// `$args[$name]`.
    pub fn arg(&self, name: &str) -> Option<&PhpValue> {
        get(&self.args, name)
    }
}

/// The ports `register_meta()` asks.
pub struct RegisterPorts<'a> {
    pub revisions: &'a dyn RevisionSupport,
    pub schema: &'a dyn DefaultSchemaCheck,
    pub callables: &'a dyn Callables,
}

const ARRAY_NEEDS_ITEMS: &str = "When registering an \"array\" meta type to show in the REST API, you must specify the schema for each array item in \"show_in_rest.schema.items\".";
const TYPE_NO_REVISIONS: &str =
    "Meta keys cannot enable revisions support unless the object type supports revisions.";
const SUBTYPE_NO_REVISIONS: &str =
    "Meta keys cannot enable revisions support unless the object subtype supports revisions.";
const DEFAULT_TYPE_MISMATCH: &str =
    "When registering a default meta value the data must match the type provided.";

/// `register_meta()`'s `$defaults`, in its order, `default` included (the `register_meta_args` filter sees it).
fn defaults() -> Vec<(PhpKey, PhpValue)> {
    let s = |v: &str| PhpValue::Str(v.as_bytes().to_vec());
    [
        ("object_subtype", s("")),
        ("type", s("string")),
        ("label", s("")),
        ("description", s("")),
        ("default", s("")),
        ("single", PhpValue::Bool(false)),
        ("sanitize_callback", PhpValue::Null),
        ("auth_callback", PhpValue::Null),
        ("show_in_rest", PhpValue::Bool(false)),
        ("revisions_enabled", PhpValue::Bool(false)),
    ]
    .into_iter()
    .map(|(k, v)| (PhpKey::Str(k.as_bytes().to_vec()), v))
    .collect()
}

/// `_wp_register_meta_args_allowed_list()`, which WordPress hooks onto `register_meta_args` by default:
/// `array_intersect_key( $args, $defaults )`, keeping the args' own order. `default` is still a default here.
pub fn allowed_args(args: Vec<(PhpKey, PhpValue)>) -> Vec<(PhpKey, PhpValue)> {
    let defaults = defaults();
    args.into_iter()
        .filter(|(k, _)| defaults.iter().any(|(d, _)| d == k))
        .collect()
}

/// A subtype as the `$wp_meta_keys` key and the hook-name text it becomes.
fn subtype_name(value: &PhpValue) -> String {
    match value {
        PhpValue::Str(s) => String::from_utf8_lossy(s).into_owned(),
        PhpValue::Int(i) => i.to_string(),
        PhpValue::Bool(true) => "1".to_owned(),
        // PHP truncates a float array key to an int.
        PhpValue::Float(f) => (*f as i64).to_string(),
        _ => String::new(),
    }
}

/// PHP's `(array) $value` for a value that is not callable.
fn to_array(value: PhpValue) -> Vec<(PhpKey, PhpValue)> {
    match value {
        PhpValue::Null => Vec::new(),
        PhpValue::Array(items) => items,
        PhpValue::Object { properties, .. } => properties,
        scalar => vec![(PhpKey::Int(0), scalar)],
    }
}

/// `register_meta()` before its `register_meta_args` filter, in PHP's order: a callable `$args` (an array callable
/// such as `[obj, 'method']` included) becomes `['sanitize_callback' => $args]`, anything else `(array) $args`;
/// then a callable `$deprecated` becomes `$args['auth_callback']`.
pub fn prepare_registration(
    args: PhpValue,
    deprecated: Option<PhpValue>,
    callables: &dyn Callables,
) -> PreparedArgs {
    // There used to be individual args for sanitize and auth callbacks.
    let (mut args, has_old_sanitize_cb) = if callables.is_callable(&args) {
        (
            vec![(PhpKey::Str(b"sanitize_callback".to_vec()), args)],
            true,
        )
    } else {
        (to_array(args), false)
    };
    let mut has_old_auth_cb = false;
    if let Some(deprecated) = deprecated.filter(|d| callables.is_callable(d)) {
        set(&mut args, "auth_callback", deprecated);
        has_old_auth_cb = true;
    }
    PreparedArgs {
        args,
        has_old_sanitize_cb,
        has_old_auth_cb,
    }
}

/// register_meta() after its register_meta_args filter (the filter is phase 3's): `prepared` comes from
/// [`prepare_registration`], its `args` possibly replaced by the filter, its flags kept.
pub fn resolve_registration(
    object_type: &str,
    key: &MetaKey,
    prepared: PreparedArgs,
    ports: &RegisterPorts<'_>,
) -> Registration {
    let PreparedArgs {
        args,
        has_old_sanitize_cb,
        has_old_auth_cb,
    } = prepared;

    // unset( $defaults['default'] ); $args = wp_parse_args( $args, $defaults );
    let mut defaults = defaults();
    unset(&mut defaults, "default");
    let mut args = array_merge(defaults, args);

    // Require an item schema when registering array meta.
    let show_in_rest = get(&args, "show_in_rest")
        .cloned()
        .unwrap_or(PhpValue::Null);
    if show_in_rest != PhpValue::Bool(false)
        && get(&args, "type") == Some(&PhpValue::Str(b"array".to_vec()))
    {
        let has_items = match &show_in_rest {
            PhpValue::Array(rest) => matches!(
                isset(rest, "schema"),
                Some(PhpValue::Array(schema)) if isset(schema, "items").is_some()
            ),
            _ => false,
        };
        if !has_items {
            return rejected(ARRAY_NEEDS_ITEMS, "5.3.0", Vec::new());
        }
    }

    let subtype = match get(&args, "object_subtype") {
        Some(v) if !is_empty(v) => subtype_name(v),
        _ => String::new(),
    };
    if get(&args, "revisions_enabled").is_some_and(|v| !is_empty(v)) {
        if object_type != "post" {
            return rejected(TYPE_NO_REVISIONS, "6.4.0", Vec::new());
        } else if !subtype.is_empty() && !ports.revisions.supports_revisions(&subtype) {
            return rejected(SUBTYPE_NO_REVISIONS, "6.4.0", Vec::new());
        }
    }

    // If `auth_callback` is not provided, fall back to `is_protected_meta()`.
    if get(&args, "auth_callback").is_none_or(is_empty) {
        let fallback: &[u8] = if is_protected_key(key) {
            b"__return_false"
        } else {
            b"__return_true"
        };
        set(&mut args, "auth_callback", PhpValue::Str(fallback.to_vec()));
    }

    // Back-compat: old sanitize and auth callbacks are applied to all of an object type.
    let names = MetaHookNames::for_object_type(object_type);
    let for_subtype = (!subtype.is_empty()).then_some(subtype.as_str());
    let mut filters = Vec::new();
    if let Some(sanitize) =
        get(&args, "sanitize_callback").filter(|c| ports.callables.is_callable(c))
    {
        filters.push(FilterToAdd {
            hook: names.sanitize(key, for_subtype),
            callback: sanitize.clone(),
            priority: 10,
            accepted_args: if for_subtype.is_some() { 4 } else { 3 },
        });
    }
    if let Some(auth) = get(&args, "auth_callback").filter(|c| ports.callables.is_callable(c)) {
        filters.push(FilterToAdd {
            hook: names.auth(key, for_subtype),
            callback: auth.clone(),
            priority: 10,
            accepted_args: 6,
        });
    }

    let mut add_default_filter = false;
    if let Some(default) = get(&args, "default") {
        let mut schema = args.clone();
        if let PhpValue::Array(rest) = &show_in_rest {
            // PHP's array_merge() throws on a non-array schema; only an array one is merged here.
            if let Some(PhpValue::Array(rest_schema)) = isset(rest, "schema") {
                schema = array_merge(schema, rest_schema.clone());
            }
        }
        if !ports
            .schema
            .default_matches(default, &PhpValue::Array(schema))
        {
            return rejected(DEFAULT_TYPE_MISMATCH, "5.5.0", filters);
        }
        add_default_filter = true;
    }

    // Global registry only contains meta keys registered with the array of arguments added in 4.6.0.
    if !has_old_auth_cb && !has_old_sanitize_cb {
        unset(&mut args, "object_subtype");
        return Registration::Registered {
            key: RegisteredMetaKey {
                object_type: object_type.to_owned(),
                subtype,
                key: key.clone(),
                args,
            },
            filters,
            add_default_filter,
        };
    }
    Registration::HooksOnly {
        filters,
        add_default_filter,
    }
}

fn rejected(message: &str, version: &'static str, filters: Vec<FilterToAdd>) -> Registration {
    Registration::Rejected {
        message: message.to_owned(),
        version,
        filters,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Yes;
    impl RevisionSupport for Yes {
        fn supports_revisions(&self, _: &str) -> bool {
            true
        }
    }
    impl DefaultSchemaCheck for Yes {
        fn default_matches(&self, _: &PhpValue, _: &PhpValue) -> bool {
            true
        }
    }
    impl Callables for Yes {
        fn is_callable(&self, value: &PhpValue) -> bool {
            matches!(value, PhpValue::Str(_))
        }
    }

    fn k(v: &str) -> PhpKey {
        PhpKey::Str(v.as_bytes().to_vec())
    }

    fn s(v: &str) -> PhpValue {
        PhpValue::Str(v.as_bytes().to_vec())
    }

    fn register(args: Vec<(PhpKey, PhpValue)>) -> Registration {
        let ports = RegisterPorts {
            revisions: &Yes,
            schema: &Yes,
            callables: &Yes,
        };
        let prepared = prepare_registration(PhpValue::Array(args), None, &Yes);
        resolve_registration("post", &MetaKey::from("color"), prepared, &ports)
    }

    #[test]
    fn subtype_zero_is_empty() {
        let Registration::Registered { key, filters, .. } =
            register(vec![(k("object_subtype"), s("0"))])
        else {
            panic!("registered");
        };
        assert_eq!(key.subtype, "");
        assert_eq!(filters[0].hook, b"auth_post_meta_color");
    }

    #[test]
    fn auth_callback_zero_falls_back() {
        let Registration::Registered { key, .. } = register(vec![(k("auth_callback"), s("0"))])
        else {
            panic!("registered");
        };
        assert_eq!(key.arg("auth_callback"), Some(&s("__return_true")));
    }

    #[test]
    fn int_keys_are_renumbered_after_the_defaults() {
        let Registration::Registered { key, .. } = register(vec![(PhpKey::Int(5), s("x"))]) else {
            panic!("registered");
        };
        assert_eq!(key.args.last(), Some(&(PhpKey::Int(0), s("x"))));
    }

    #[test]
    fn allowed_args_keep_default_and_drop_the_rest() {
        let kept = allowed_args(vec![
            (k("foo"), s("1")),
            (k("default"), s("2")),
            (PhpKey::Int(0), s("3")),
            (k("type"), s("4")),
        ]);
        assert_eq!(kept, vec![(k("default"), s("2")), (k("type"), s("4"))]);
    }

    #[test]
    fn single_and_default() {
        let Registration::Registered { key, .. } =
            register(vec![(k("single"), s("1")), (k("default"), PhpValue::Null)])
        else {
            panic!("registered");
        };
        assert!(key.single());
        assert_eq!(key.default(), Some(&PhpValue::Null));
    }

    /// `is_callable()` true for strings and for `[class, method]` arrays.
    struct WithArrays;
    impl Callables for WithArrays {
        fn is_callable(&self, value: &PhpValue) -> bool {
            match value {
                PhpValue::Str(_) => true,
                PhpValue::Array(items) => {
                    items.len() == 2 && items.iter().all(|(_, v)| matches!(v, PhpValue::Str(_)))
                }
                _ => false,
            }
        }
    }

    fn resolve(prepared: PreparedArgs) -> Registration {
        let ports = RegisterPorts {
            revisions: &Yes,
            schema: &Yes,
            callables: &WithArrays,
        };
        resolve_registration("post", &MetaKey::from("color"), prepared, &ports)
    }

    #[test]
    fn array_callable_is_the_legacy_sanitize_form() {
        let callable = PhpValue::Array(vec![
            (PhpKey::Int(0), s("Klass")),
            (PhpKey::Int(1), s("clean")),
        ]);
        let prepared = prepare_registration(callable.clone(), None, &WithArrays);
        assert!(prepared.has_old_sanitize_cb);
        assert!(!prepared.has_old_auth_cb);
        assert_eq!(
            prepared.args,
            vec![(k("sanitize_callback"), callable.clone())]
        );
        let Registration::HooksOnly { filters, .. } = resolve(prepared) else {
            panic!("hooks only");
        };
        assert_eq!(filters[0].hook, b"sanitize_post_meta_color");
        assert_eq!(filters[0].callback, callable);
    }

    #[test]
    fn non_callable_args_are_cast_to_an_array() {
        let prepared = prepare_registration(PhpValue::Int(5), None, &Yes);
        assert_eq!(prepared.args, vec![(PhpKey::Int(0), PhpValue::Int(5))]);
        assert!(!prepared.has_old_sanitize_cb);
        assert!(
            prepare_registration(PhpValue::Null, None, &Yes)
                .args
                .is_empty()
        );
    }

    #[test]
    fn deprecated_auth_goes_into_the_args_the_filter_sees() {
        let prepared = prepare_registration(
            PhpValue::Array(vec![(k("auth_callback"), s("first"))]),
            Some(s("old_auth")),
            &Yes,
        );
        assert!(prepared.has_old_auth_cb);
        assert_eq!(prepared.args, vec![(k("auth_callback"), s("old_auth"))]);
        // A non-callable $deprecated is ignored.
        let prepared = prepare_registration(PhpValue::Array(vec![]), Some(PhpValue::Int(1)), &Yes);
        assert!(!prepared.has_old_auth_cb);
        assert!(prepared.args.is_empty());
    }

    #[test]
    fn old_flags_survive_a_filter_that_changes_args() {
        // A register_meta_args filter that throws the args away and returns its own.
        let mut prepared =
            prepare_registration(PhpValue::Array(vec![]), Some(s("old_auth")), &WithArrays);
        prepared.args = vec![(k("single"), PhpValue::Bool(true))];
        assert!(matches!(resolve(prepared), Registration::HooksOnly { .. }));

        let mut prepared = prepare_registration(s("old_sanitize"), None, &WithArrays);
        prepared.args = allowed_args(vec![(k("foo"), s("bar"))]);
        assert!(prepared.args.is_empty());
        let Registration::HooksOnly { filters, .. } = resolve(prepared) else {
            panic!("hooks only");
        };
        // The filter dropped the sanitize callback, so only the auth fallback is added.
        assert_eq!(filters.len(), 1);
        assert_eq!(filters[0].callback, s("__return_true"));
    }
}
