//! PHP values as `serialize()` writes them.

/// An array key or property name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PhpKey {
    Int(i64),
    Str(Vec<u8>),
}

/// A value in PHP's serialize format.
#[derive(Debug, Clone)]
pub enum PhpValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(Vec<u8>),
    Array(Vec<(PhpKey, PhpValue)>),
    Object {
        class: String,
        properties: Vec<(PhpKey, PhpValue)>,
    },
    /// `C:`: a `Serializable` object's own payload.
    Custom {
        class: String,
        data: Vec<u8>,
    },
    /// `E:`: an enum case.
    Enum {
        class: String,
        case: String,
    },
    /// `R:n`: a PHP reference to slot `n`.
    Ref(u32),
    /// `r:n`: the same object as slot `n`.
    ObjectRef(u32),
}

impl PartialEq for PhpValue {
    /// Floats compare bitwise, so `NAN` equals itself and `-0.0` differs from `0.0`.
    fn eq(&self, other: &Self) -> bool {
        use PhpValue::*;
        match (self, other) {
            (Null, Null) => true,
            (Bool(a), Bool(b)) => a == b,
            (Int(a), Int(b)) => a == b,
            (Float(a), Float(b)) => a.to_bits() == b.to_bits(),
            (Str(a), Str(b)) => a == b,
            (Array(a), Array(b)) => a == b,
            (
                Object {
                    class: c1,
                    properties: p1,
                },
                Object {
                    class: c2,
                    properties: p2,
                },
            ) => c1 == c2 && p1 == p2,
            (
                Custom {
                    class: c1,
                    data: d1,
                },
                Custom {
                    class: c2,
                    data: d2,
                },
            ) => c1 == c2 && d1 == d2,
            (
                Enum {
                    class: c1,
                    case: k1,
                },
                Enum {
                    class: c2,
                    case: k2,
                },
            ) => c1 == c2 && k1 == k2,
            (Ref(a), Ref(b)) => a == b,
            (ObjectRef(a), ObjectRef(b)) => a == b,
            _ => false,
        }
    }
}

impl PhpValue {
    /// True when an object (`O:`, `C:`, `E:` or `r:`) is anywhere inside.
    pub fn contains_object(&self) -> bool {
        match self {
            PhpValue::Object { .. }
            | PhpValue::Custom { .. }
            | PhpValue::Enum { .. }
            | PhpValue::ObjectRef(_) => true,
            PhpValue::Array(items) => items.iter().any(|(_, v)| v.contains_object()),
            _ => false,
        }
    }
}
