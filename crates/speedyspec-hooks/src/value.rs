use std::any::Any;
use std::fmt;
use std::rc::Rc;

use serde::de::Deserializer;
use serde::ser::{Error as _, Serializer};
use serde::{Deserialize, Serialize};

/// An array key. PHP arrays key by integer or string, and the difference is
/// observable, so it is kept rather than flattened to a string.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Key {
    Int(i64),
    Str(String),
}

/// A value passed through hooks, shaped like a PHP value.
///
/// `Opaque` exists so a Rust callback and a PHP callback in the same process can
/// hand each other something neither looks inside (a PHP object, say). It is
/// deliberately not portable: a backend that has to leave the process cannot
/// carry it.
#[derive(Clone)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    /// Only for strings that are not valid UTF-8.
    Bytes(Vec<u8>),
    /// Ordered, like a PHP array.
    Array(Vec<(Key, Value)>),
    /// Carried, never looked inside; cannot leave the process.
    Opaque(Rc<dyn Any>),
}

impl Value {
    /// False if an `Opaque` is anywhere inside, i.e. the value cannot be
    /// serialised and so cannot be moved to another process.
    pub fn is_portable(&self) -> bool {
        match self {
            Value::Opaque(_) => false,
            Value::Array(items) => items.iter().all(|(_, v)| v.is_portable()),
            _ => true,
        }
    }
}

impl PartialEq for Value {
    /// Two `Opaque` values are equal only when they are the same `Rc`.
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Bytes(a), Value::Bytes(b)) => a == b,
            (Value::Array(a), Value::Array(b)) => a == b,
            (Value::Opaque(a), Value::Opaque(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => f.write_str("Null"),
            Value::Bool(v) => f.debug_tuple("Bool").field(v).finish(),
            Value::Int(v) => f.debug_tuple("Int").field(v).finish(),
            Value::Float(v) => f.debug_tuple("Float").field(v).finish(),
            Value::Str(v) => f.debug_tuple("Str").field(v).finish(),
            Value::Bytes(v) => f.debug_tuple("Bytes").field(v).finish(),
            Value::Array(v) => f.debug_tuple("Array").field(v).finish(),
            Value::Opaque(_) => f.write_str("Opaque(..)"),
        }
    }
}

// The serialised shape is an externally tagged enum, e.g. "Null", {"Int":3} or
// {"Array":[[{"Str":"k"},"Null"]]}. The tag keeps Int/Float and Str/Bytes
// apart; the list of pairs keeps key type and order. Variant names and order
// here must match `Repr`, which is what deserialisation reads.
impl Serialize for Value {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        const NAME: &str = "Repr";
        match self {
            Value::Null => serializer.serialize_unit_variant(NAME, 0, "Null"),
            Value::Bool(v) => serializer.serialize_newtype_variant(NAME, 1, "Bool", v),
            Value::Int(v) => serializer.serialize_newtype_variant(NAME, 2, "Int", v),
            Value::Float(v) => serializer.serialize_newtype_variant(NAME, 3, "Float", v),
            Value::Str(v) => serializer.serialize_newtype_variant(NAME, 4, "Str", v),
            Value::Bytes(v) => serializer.serialize_newtype_variant(NAME, 5, "Bytes", v),
            Value::Array(v) => serializer.serialize_newtype_variant(NAME, 6, "Array", v),
            Value::Opaque(_) => Err(S::Error::custom(
                "an Opaque value cannot be serialised: it cannot leave the process",
            )),
        }
    }
}

/// Mirror of the portable variants of [`Value`], used only to derive the
/// deserialiser.
#[derive(Deserialize)]
enum Repr {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Bytes(Vec<u8>),
    Array(Vec<(Key, Value)>),
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match Repr::deserialize(deserializer)? {
            Repr::Null => Value::Null,
            Repr::Bool(v) => Value::Bool(v),
            Repr::Int(v) => Value::Int(v),
            Repr::Float(v) => Value::Float(v),
            Repr::Str(v) => Value::Str(v),
            Repr::Bytes(v) => Value::Bytes(v),
            Repr::Array(v) => Value::Array(v),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Value {
        Value::Array(vec![
            (Key::Int(0), Value::Int(1)),
            (Key::Str("0".into()), Value::Float(1.0)),
            (Key::Int(5), Value::Str("x".into())),
            (Key::Str("b".into()), Value::Bytes(vec![0xff, 0xfe])),
            (
                Key::Str("n".into()),
                Value::Array(vec![
                    (Key::Int(1), Value::Null),
                    (Key::Int(0), Value::Bool(true)),
                ]),
            ),
        ])
    }

    #[test]
    fn portable_value_serialises_and_back() {
        let v = sample();
        let json = serde_json::to_string(&v).unwrap();
        let back: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v, back);
    }

    #[test]
    fn opaque_refuses_to_serialise() {
        let v = Value::Opaque(Rc::new(1u8));
        assert!(serde_json::to_string(&v).is_err());
        let nested = Value::Array(vec![(Key::Int(0), v)]);
        assert!(serde_json::to_string(&nested).is_err());
    }

    #[test]
    fn is_portable_sees_nested_opaque() {
        let v = Value::Array(vec![(
            Key::Int(0),
            Value::Array(vec![(Key::Int(0), Value::Opaque(Rc::new(1u8)))]),
        )]);
        assert!(!v.is_portable());
        assert!(sample().is_portable());
    }
}
