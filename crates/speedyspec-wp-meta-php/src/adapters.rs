//! The metadata API's outbound ports over PHP's WordPress functions (Adapter), kept until the seams that own posts,
//! terms, comments, users and sites bind Rust ones.
//!
//! A port whose contract cannot fail (it answers `bool`, `String` or `Option`) parks the failure it met ([`park`],
//! [`park_failure`]). From then on the use case stops where WordPress would have: [`PhpValueHooks`] refuses every
//! hook while something is parked, and `register_meta()` checks after each step that asks a port, so nothing is
//! filtered, fired or written after the failure. The native function that is running then rethrows what was parked
//! ([`take_parked`]).
//!
//! [`PhpValueHooks`]: crate::PhpValueHooks

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use speedyspec_container::Container;
use speedyspec_data::Statement;
use speedyspec_php_ext::{Key, Value};
use speedyspec_php_serialize::PhpValue;
use speedyspec_php_serialize::semantics::bytes_truthy;
use speedyspec_wp_db_contracts::{Field, QueryOutcome, WpDbError, WpValue, Wpdb};
use speedyspec_wp_meta::Sites;
use speedyspec_wp_meta_contracts::{
    Callables, DefaultSchemaCheck, Installing, MetaKey, MetaKeys, MetaRepositories, MetaRepository,
    MetaType, ObjectId, ObjectSubtypes, RegisteredMetaKey, RevisionSupport,
};

use crate::codec::{Thrown, from_php_live, php, string, to_php};

/// What a port met that its contract cannot report.
pub(crate) enum Parked {
    /// A PHP exception, rethrown as the same object.
    Exception(Value),
    /// A failure with no PHP exception behind it (a container binding that did not resolve), thrown as `class`.
    Failure {
        class: &'static str,
        message: String,
    },
}

thread_local! {
    static PARKED: RefCell<Option<Parked>> = const { RefCell::new(None) };
}

/// Parks `parked` unless something is parked already (the first failure wins).
pub(crate) fn keep(parked: Parked) {
    PARKED.with(|p| {
        p.borrow_mut().get_or_insert(parked);
    });
}

/// Keeps the first exception a port met while a native function runs.
pub(crate) fn park(thrown: Thrown) {
    if let Some(exception) = thrown.0 {
        keep(Parked::Exception(exception));
    }
}

/// Keeps a failure that has no PHP exception behind it.
pub(crate) fn park_failure(class: &'static str, message: String) {
    keep(Parked::Failure { class, message });
}

/// Whether a port has parked a failure the running native function has not rethrown yet.
pub(crate) fn is_parked() -> bool {
    PARKED.with(|p| p.borrow().is_some())
}

/// What a port parked, taken once.
pub(crate) fn take_parked() -> Option<Parked> {
    PARKED.with(|p| p.borrow_mut().take())
}

/// Calls a PHP function; an exception is parked and the answer is `None`.
///
/// Once something is parked, no more PHP runs: the answer is `None` at once, as WordPress would not have got here.
pub(crate) fn ask(name: &str, args: &[Value]) -> Option<Value> {
    if is_parked() {
        return None;
    }
    match php(name, args) {
        Ok(v) => Some(v),
        Err(thrown) => {
            park(thrown);
            None
        }
    }
}

/// `function_exists( $name )`.
pub(crate) fn exists(name: &str) -> bool {
    !is_parked()
        && matches!(
            php("function_exists", &[Value::Str(name.into())]),
            Ok(Value::Bool(true))
        )
}

/// PHP's truthiness of a PHP value.
pub(crate) fn truthy(value: &Value) -> bool {
    match &*value.read() {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Int(i) => *i != 0,
        Value::Float(f) => *f != 0.0,
        Value::Str(s) => bytes_truthy(s.as_bytes()),
        Value::Bytes(b) => bytes_truthy(b),
        Value::Array(items) => !items.is_empty(),
        Value::Opaque(_) => true,
    }
}

const HELPERS: &str = "SpeedySpec\\WpMeta\\Php\\";

/// `wp_installing()`, false when it is not declared.
pub struct PhpInstalling;

impl Installing for PhpInstalling {
    fn installing(&self) -> bool {
        exists("wp_installing") && ask("wp_installing", &[]).is_some_and(|v| truthy(&v))
    }
}

/// `get_object_subtype()`'s switch for posts, terms, comments and users, as WordPress 7.2-RC2 has it: the post type,
/// the term's taxonomy, `'comment'`, and `'user'` when `get_userdata()` finds the user (Task 3's recording).
pub struct PhpObjectSubtypes;

impl ObjectSubtypes for PhpObjectSubtypes {
    fn subtype_of(&self, meta_type: &MetaType, object: ObjectId) -> String {
        let id = Value::Int(i64::try_from(object.0).unwrap_or(i64::MAX));
        match ask(
            &format!("{HELPERS}object_subtype"),
            &[Value::Str(meta_type.name().into()), id],
        ) {
            Some(Value::Str(s)) => s,
            Some(Value::Bytes(b)) => String::from_utf8_lossy(&b).into_owned(),
            _ => String::new(),
        }
    }
}

/// `is_multisite()` and `get_site()`, for the `'blog'` subtype.
pub struct PhpSites;

impl Sites for PhpSites {
    fn is_multisite(&self) -> bool {
        exists("is_multisite") && ask("is_multisite", &[]).is_some_and(|v| truthy(&v))
    }

    fn site_exists(&self, id: ObjectId) -> bool {
        exists("get_site")
            && ask(
                "get_site",
                &[Value::Int(i64::try_from(id.0).unwrap_or(i64::MAX))],
            )
            .is_some_and(|v| truthy(&v))
    }
}

/// `post_type_supports( $post_type, 'revisions' )`.
pub struct PhpRevisionSupport;

impl RevisionSupport for PhpRevisionSupport {
    fn supports_revisions(&self, post_type: &str) -> bool {
        exists("post_type_supports")
            && ask(
                "post_type_supports",
                &[Value::Str(post_type.into()), Value::Str("revisions".into())],
            )
            .is_some_and(|v| truthy(&v))
    }
}

/// `is_callable( $value )`.
pub struct PhpCallables;

impl Callables for PhpCallables {
    fn is_callable(&self, value: &PhpValue) -> bool {
        match to_php(value) {
            Ok(v) => ask("is_callable", &[v]).is_some_and(|v| truthy(&v)),
            Err(thrown) => {
                park(thrown);
                false
            }
        }
    }
}

/// `! is_wp_error( rest_validate_value_from_schema( $default, $schema ) )`; true while rest-api.php is not loaded.
pub struct PhpDefaultSchemaCheck;

impl DefaultSchemaCheck for PhpDefaultSchemaCheck {
    fn default_matches(&self, default: &PhpValue, schema: &PhpValue) -> bool {
        if !exists("rest_validate_value_from_schema") {
            return true;
        }
        let (default, schema) = match (to_php(default), to_php(schema)) {
            (Ok(d), Ok(s)) => (d, s),
            (Err(thrown), _) | (_, Err(thrown)) => {
                park(thrown);
                return false;
            }
        };
        match ask("rest_validate_value_from_schema", &[default, schema]) {
            Some(check) => !ask("is_wp_error", &[check]).is_some_and(|v| truthy(&v)),
            None => false,
        }
    }
}

fn text(key: &Key) -> String {
    match key {
        Key::Int(i) => i.to_string(),
        Key::Str(s) => s.clone(),
    }
}

fn key_bytes(key: &Key) -> Vec<u8> {
    text(key).into_bytes()
}

/// `$wp_meta_keys`, read live (Registry): `register_meta()` and PHP code write it, and every read sees their writes.
pub struct PhpMetaKeys;

impl PhpMetaKeys {
    /// `$wp_meta_keys[ $object_type ]`, as a list of (subtype, keys array).
    fn subtypes_of(object_type: &str) -> Vec<(String, Value)> {
        match speedyspec_php::global_array_get("wp_meta_keys", object_type.as_bytes())
            .map(|v| v.read().into_owned())
        {
            Some(Value::Array(items)) => items
                .into_iter()
                .map(|(k, v)| (text(&k), v.read().into_owned()))
                .collect(),
            _ => Vec::new(),
        }
    }

    fn entries(object_type: &str, subtype: &str) -> Vec<(Key, Value)> {
        Self::subtypes_of(object_type)
            .into_iter()
            .find(|(s, _)| s == subtype)
            .map(|(_, keys)| match keys {
                Value::Array(items) => items,
                _ => Vec::new(),
            })
            .unwrap_or_default()
    }

    fn registered(object_type: &str, subtype: &str, key: &Key, args: &Value) -> RegisteredMetaKey {
        let args = match from_php_live(args) {
            PhpValue::Array(items) => items,
            _ => Vec::new(),
        };
        RegisteredMetaKey {
            object_type: object_type.to_owned(),
            subtype: subtype.to_owned(),
            key: MetaKey::new(key_bytes(key)),
            args,
        }
    }
}

/// `$array[ $key ]`'s key for a meta key: PHP stores `"10"` as the int `10`.
fn same_key(stored: &Key, key: &MetaKey) -> bool {
    key_bytes(stored) == key.as_bytes()
}

impl MetaKeys for PhpMetaKeys {
    fn keys(&self, object_type: &str, subtype: &str) -> Vec<RegisteredMetaKey> {
        Self::entries(object_type, subtype)
            .iter()
            .map(|(k, args)| Self::registered(object_type, subtype, k, args))
            .collect()
    }

    fn find(&self, object_type: &str, subtype: &str, key: &MetaKey) -> Option<RegisteredMetaKey> {
        Self::entries(object_type, subtype)
            .iter()
            .find(|(k, _)| same_key(k, key))
            .map(|(k, args)| Self::registered(object_type, subtype, k, args))
    }

    fn subtypes(&self, object_type: &str) -> Vec<String> {
        Self::subtypes_of(object_type)
            .into_iter()
            .map(|(s, _)| s)
            .collect()
    }
}

/// `$wpdb` with `$wpdb->{type}meta` read from PHP when native `wpdb` does not know the table (Decorator): a plugin's
/// custom type declares its table as a dynamic property (`$wpdb->thingmeta = 'wp_thingmeta'`), which only PHP holds.
pub struct PhpTableNames {
    inner: Rc<dyn Wpdb>,
}

impl PhpTableNames {
    pub fn new(inner: Rc<dyn Wpdb>) -> PhpTableNames {
        PhpTableNames { inner }
    }
}

impl Wpdb for PhpTableNames {
    fn table_name(&self, logical: &str) -> String {
        let name = self.inner.table_name(logical);
        if !name.is_empty() {
            return name;
        }
        match php(&format!("{HELPERS}table"), &[Value::Str(logical.into())]) {
            Ok(Value::Str(s)) => s,
            Ok(_) => String::new(),
            Err(thrown) => {
                park(thrown);
                String::new()
            }
        }
    }

    fn query_statement(&self, stmt: Statement) -> Result<QueryOutcome, WpDbError> {
        self.inner.query_statement(stmt)
    }

    fn query_bytes(&self, sql: &[u8]) -> Result<QueryOutcome, WpDbError> {
        self.inner.query_bytes(sql)
    }

    fn prepare(&self, query: Option<&[u8]>, args: &[WpValue]) -> Option<Vec<u8>> {
        self.inner.prepare(query, args)
    }

    fn process_fields(
        &self,
        table: &str,
        data: &[(String, WpValue)],
        format: Option<&[String]>,
    ) -> Result<Vec<Field>, WpDbError> {
        self.inner.process_fields(table, data, format)
    }

    fn insert(
        &self,
        table: &str,
        data: &[(String, WpValue)],
        format: Option<&[String]>,
    ) -> Result<u64, WpDbError> {
        self.inner.insert(table, data, format)
    }

    fn update(
        &self,
        table: &str,
        data: &[(String, WpValue)],
        conditions: &[(String, WpValue)],
        format: Option<&[String]>,
        where_format: Option<&[String]>,
    ) -> Result<u64, WpDbError> {
        self.inner
            .update(table, data, conditions, format, where_format)
    }

    fn delete(
        &self,
        table: &str,
        conditions: &[(String, WpValue)],
        where_format: Option<&[String]>,
    ) -> Result<u64, WpDbError> {
        self.inner.delete(table, conditions, where_format)
    }

    fn insert_id(&self) -> u64 {
        self.inner.insert_id()
    }

    fn suppress_errors(&self, suppress: bool) -> bool {
        self.inner.suppress_errors(suppress)
    }

    fn charset(&self) -> String {
        self.inner.charset()
    }
}

/// The container key of the meta repositories port: PHP binds an object implementing the
/// `SpeedySpec\WpMeta\MetaRepositories` twin here to replace the SQL tables.
pub const META_REPOSITORIES: &str = "SpeedySpec\\WpMeta\\MetaRepositories";

/// The repositories resolved from the container on every call, so a binding PHP made for the request applies; the
/// default binding (the SQL tables) when the container cannot resolve one. The container is held weakly: it holds
/// this adapter's owners.
pub struct ResolvedRepositories {
    container: Weak<Container>,
    fallback: Rc<dyn MetaRepositories>,
}

impl ResolvedRepositories {
    pub fn new(
        container: &Rc<Container>,
        fallback: Rc<dyn MetaRepositories>,
    ) -> ResolvedRepositories {
        ResolvedRepositories {
            container: Rc::downgrade(container),
            fallback,
        }
    }
}

impl MetaRepositories for ResolvedRepositories {
    fn for_type(&self, meta_type: &MetaType) -> Option<Rc<dyn MetaRepository>> {
        // Only a container that is gone falls back to the SQL tables: the default binding always resolves, so a
        // failure is a PHP binding that threw or does not implement the twin, and it must reach the caller.
        let Some(container) = self.container.upgrade() else {
            return self.fallback.for_type(meta_type);
        };
        match container.make_port::<dyn MetaRepositories>(META_REPOSITORIES) {
            Ok(repositories) => repositories.for_type(meta_type),
            Err(err) => {
                park_failure("SpeedySpec\\Container\\ContainerException", err.to_string());
                None
            }
        }
    }
}

/// A PHP string or `false`, as `_get_meta_table()` answers.
pub(crate) fn table_or_false(name: String) -> Value {
    if name.is_empty() {
        Value::Bool(false)
    } else {
        string(name.as_bytes())
    }
}

/// `$value` as a list of PHP ints.
pub(crate) fn int_list(ids: impl Iterator<Item = u64>) -> Value {
    Value::Array(
        ids.enumerate()
            .map(|(i, id)| {
                (
                    Key::Int(i as i64),
                    Value::Int(i64::try_from(id).unwrap_or(i64::MAX)),
                )
            })
            .collect(),
    )
}
