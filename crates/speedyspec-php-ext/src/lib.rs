//! The speedyspec_wordpress extension's shared parts.
//!
//! - [`php_port`] turns a Rust port trait into a PHP *twin* (an interface with the same methods) and a PHP adapter
//!   (`Php<Trait>`), a Rust struct that implements the trait by calling a PHP object implementing the twin.
//! - [`twins_area`] declares every twin at the start of each request.
//! - [`ToPhp`] and [`FromPhp`] convert the values that cross that boundary.
//! - [`boundary_crossings`] counts entries into PHP, so tests can show a Rust path never entered it.

use std::rc::Rc;

pub use inventory;
pub use speedyspec_hooks::{Key, Value};
pub use speedyspec_php::{NativeArea, PhpError};
pub use speedyspec_php_port_macros::php_port;

/// How many times this process has entered PHP from Rust. A Rust caller reaching a Rust adapter must leave it
/// unchanged.
pub fn boundary_crossings() -> u64 {
    speedyspec_php::php_entries()
}

/// A PHP object held by Rust, valid for the request that made it.
#[derive(Clone)]
pub struct PhpObject(Value);

impl PhpObject {
    /// `None` unless `value` is a PHP object (a `Value::Opaque` from PHP).
    pub fn new(value: Value) -> Option<PhpObject> {
        matches!(value, Value::Opaque(_)).then_some(PhpObject(value))
    }

    /// Calls `$object->method(...$args)`.
    pub fn call(&self, method: &str, args: Vec<Value>) -> Result<Value, PhpError> {
        let callable = Value::Array(vec![
            (Key::Int(0), self.0.clone()),
            (Key::Int(1), Value::Str(method.to_owned())),
        ]);
        speedyspec_php::call(&callable, &args)
    }

    pub fn value(&self) -> &Value {
        &self.0
    }
}

/// A Rust value passed to PHP.
pub trait ToPhp {
    fn to_php(self) -> Value;
}

/// A value PHP returned, or what a PHP adapter answers when the call into PHP failed.
pub trait FromPhp: Sized {
    fn from_php(value: Value) -> Result<Self, String>;

    /// What a PHP adapter's method returns when the PHP call failed. A port whose PHP side can fail returns
    /// `Result<_, PortError>`, which carries the failure; any other return type means the port was declared
    /// infallible, and a failure is a broken PHP adapter.
    fn from_failure(port: &str, method: &str, why: String) -> Self {
        panic!(
            "the PHP adapter of {port}::{method} failed, and the port cannot report errors: {why}"
        )
    }
}

/// A port call that failed on the PHP side.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortError(pub String);

impl std::fmt::Display for PortError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PortError {}

impl ToPhp for Value {
    fn to_php(self) -> Value {
        self
    }
}

impl FromPhp for Value {
    fn from_php(value: Value) -> Result<Self, String> {
        Ok(value)
    }
}

impl ToPhp for String {
    fn to_php(self) -> Value {
        Value::Str(self)
    }
}

impl FromPhp for String {
    fn from_php(value: Value) -> Result<Self, String> {
        match value {
            Value::Str(s) => Ok(s),
            other => Err(format!("expected a string, PHP returned {other:?}")),
        }
    }
}

impl ToPhp for i64 {
    fn to_php(self) -> Value {
        Value::Int(self)
    }
}

impl FromPhp for i64 {
    fn from_php(value: Value) -> Result<Self, String> {
        match value {
            Value::Int(n) => Ok(n),
            other => Err(format!("expected an int, PHP returned {other:?}")),
        }
    }
}

impl ToPhp for f64 {
    fn to_php(self) -> Value {
        Value::Float(self)
    }
}

impl FromPhp for f64 {
    fn from_php(value: Value) -> Result<Self, String> {
        match value {
            Value::Float(n) => Ok(n),
            Value::Int(n) => Ok(n as f64),
            other => Err(format!("expected a float, PHP returned {other:?}")),
        }
    }
}

impl ToPhp for bool {
    fn to_php(self) -> Value {
        Value::Bool(self)
    }
}

impl FromPhp for bool {
    fn from_php(value: Value) -> Result<Self, String> {
        match value {
            Value::Bool(b) => Ok(b),
            other => Err(format!("expected a bool, PHP returned {other:?}")),
        }
    }
}

impl FromPhp for () {
    fn from_php(_value: Value) -> Result<Self, String> {
        Ok(())
    }
}

impl<T: ToPhp> ToPhp for Vec<T> {
    fn to_php(self) -> Value {
        Value::Array(
            self.into_iter()
                .enumerate()
                .map(|(i, v)| (Key::Int(i as i64), v.to_php()))
                .collect(),
        )
    }
}

impl<T: FromPhp> FromPhp for Vec<T> {
    fn from_php(value: Value) -> Result<Self, String> {
        match value {
            Value::Array(entries) => entries.into_iter().map(|(_, v)| T::from_php(v)).collect(),
            other => Err(format!("expected an array, PHP returned {other:?}")),
        }
    }
}

impl<T: ToPhp> ToPhp for Option<T> {
    fn to_php(self) -> Value {
        self.map_or(Value::Null, ToPhp::to_php)
    }
}

impl<T: FromPhp> FromPhp for Option<T> {
    fn from_php(value: Value) -> Result<Self, String> {
        match value {
            Value::Null => Ok(None),
            other => T::from_php(other).map(Some),
        }
    }
}

impl<T: FromPhp> FromPhp for Result<T, PortError> {
    fn from_php(value: Value) -> Result<Self, String> {
        Ok(T::from_php(value).map_err(PortError))
    }

    fn from_failure(port: &str, method: &str, why: String) -> Self {
        Err(PortError(format!("{port}::{method}: {why}")))
    }
}

/// A PHP twin: the PHP source of a port's interface, submitted by [`php_port`].
pub struct Twin {
    pub name: &'static str,
    pub source: &'static str,
}

inventory::collect!(Twin);

/// Every twin linked into this binary, by PHP name.
pub fn twins() -> Vec<&'static Twin> {
    let mut all: Vec<&'static Twin> = inventory::iter::<Twin>.into_iter().collect();
    all.sort_by_key(|twin| twin.name);
    all
}

/// The area that declares every twin at the start of each request, before any other PHP code.
pub fn twins_area() -> Rc<dyn NativeArea> {
    Rc::new(TwinsArea)
}

struct TwinsArea;

impl NativeArea for TwinsArea {
    fn request_start(&self) {
        for twin in twins() {
            if let Err(err) =
                speedyspec_php::eval(&format!("speedyspec://twin/{}", twin.name), twin.source)
            {
                eprintln!(
                    "speedyspec_wordpress: declaring the PHP twin {} failed: {err}",
                    twin.name
                );
            }
        }
    }
}
