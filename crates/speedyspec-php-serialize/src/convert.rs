//! Conversions between PHP's serialized values and the Hook API's `Value`.

use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use speedyspec_hooks::{Key, Value};

use crate::{PhpKey, PhpValue};

/// What a `Value` cannot represent (objects, cycles, keys that are not UTF-8), carried in `Value::Opaque`.
#[derive(Debug, Clone, PartialEq)]
pub struct OpaquePhpValue(pub PhpValue);

/// A `Value` that cannot become PHP's serialized form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConvertError {
    /// An opaque value from somewhere else (a live PHP object, a Rust value).
    Foreign,
}

impl fmt::Display for ConvertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the value holds something that has no serialized form")
    }
}

impl std::error::Error for ConvertError {}

/// Slots numbered as PHP's `var_hash` numbers them: from 1, in order, one per value; references take none.
fn number<'a>(value: &'a PhpValue, slots: &mut Vec<&'a PhpValue>) {
    if matches!(value, PhpValue::Ref(_) | PhpValue::ObjectRef(_)) {
        return;
    }
    slots.push(value);
    if let PhpValue::Array(items)
    | PhpValue::Object {
        properties: items, ..
    } = value
    {
        for (_, v) in items {
            number(v, slots);
        }
    }
}

struct Converter<'a> {
    slots: Vec<&'a PhpValue>,
    next: usize,
    objects: HashMap<usize, Rc<OpaquePhpValue>>,
    ancestors: Vec<usize>,
}

/// Why a value has to stay whole.
struct Whole;

fn string(s: &[u8]) -> Value {
    match std::str::from_utf8(s) {
        Ok(t) => Value::Str(t.to_string()),
        Err(_) => Value::Bytes(s.to_vec()),
    }
}

impl<'a> Converter<'a> {
    fn convert(&mut self, value: &'a PhpValue) -> Result<Value, Whole> {
        match value {
            PhpValue::Ref(n) => {
                let n = *n as usize;
                if self.ancestors.contains(&n) {
                    return Err(Whole);
                }
                let target = *self.slots.get(n.wrapping_sub(1)).ok_or(Whole)?;
                // A copy, converted on its own: a reference takes no slot.
                Converter {
                    slots: Vec::new(),
                    next: 0,
                    objects: HashMap::new(),
                    ancestors: Vec::new(),
                }
                .detached(target)
            }
            PhpValue::ObjectRef(n) => self
                .objects
                .get(&(*n as usize))
                .cloned()
                .map(|rc| Value::Opaque(rc))
                .ok_or(Whole),
            _ => {
                self.next += 1;
                let slot = self.next;
                match value {
                    PhpValue::Null => Ok(Value::Null),
                    PhpValue::Bool(b) => Ok(Value::Bool(*b)),
                    PhpValue::Int(i) => Ok(Value::Int(*i)),
                    PhpValue::Float(f) => Ok(Value::Float(*f)),
                    PhpValue::Str(s) => Ok(string(s)),
                    PhpValue::Array(items) => {
                        self.ancestors.push(slot);
                        let mut out = Vec::with_capacity(items.len());
                        for (k, v) in items {
                            let key = match k {
                                PhpKey::Int(i) => Key::Int(*i),
                                PhpKey::Str(s) => {
                                    Key::Str(String::from_utf8(s.clone()).map_err(|_| Whole)?)
                                }
                            };
                            out.push((key, self.convert(v)?));
                        }
                        self.ancestors.pop();
                        Ok(Value::Array(out))
                    }
                    PhpValue::Object { properties, .. } => {
                        // Its members still take their slots, so later references number as PHP does.
                        let mut members = Vec::new();
                        for (_, v) in properties {
                            number(v, &mut members);
                        }
                        self.next += members.len();
                        let rc = Rc::new(OpaquePhpValue(value.clone()));
                        self.objects.insert(slot, rc.clone());
                        Ok(Value::Opaque(rc))
                    }
                    PhpValue::Custom { .. } | PhpValue::Enum { .. } => {
                        let rc = Rc::new(OpaquePhpValue(value.clone()));
                        self.objects.insert(slot, rc.clone());
                        Ok(Value::Opaque(rc))
                    }
                    PhpValue::Ref(_) | PhpValue::ObjectRef(_) => unreachable!("handled above"),
                }
            }
        }
    }

    fn detached(mut self, value: &'a PhpValue) -> Result<Value, Whole> {
        number(value, &mut self.slots);
        self.convert(value)
    }
}

/// A serialized value as the Hook API carries it. Objects become opaque (the same `Rc` for every `r:` to one);
/// a value with a cycle or a key that is not UTF-8 becomes one opaque value whole.
pub fn to_hook_value(value: &PhpValue) -> Value {
    let mut converter = Converter {
        slots: Vec::new(),
        next: 0,
        objects: HashMap::new(),
        ancestors: Vec::new(),
    };
    number(value, &mut converter.slots);
    converter
        .convert(value)
        .unwrap_or_else(|_| Value::Opaque(Rc::new(OpaquePhpValue(value.clone()))))
}

/// A hook value in serialized form; an opaque value from elsewhere is `Foreign`.
pub fn from_hook_value(value: &Value) -> Result<PhpValue, ConvertError> {
    let value = value.read();
    Ok(match &*value {
        Value::Null => PhpValue::Null,
        Value::Bool(b) => PhpValue::Bool(*b),
        Value::Int(i) => PhpValue::Int(*i),
        Value::Float(f) => PhpValue::Float(*f),
        Value::Str(s) => PhpValue::Str(s.as_bytes().to_vec()),
        Value::Bytes(b) => PhpValue::Str(b.clone()),
        Value::Array(items) => PhpValue::Array(
            items
                .iter()
                .map(|(k, v)| {
                    let key = match k {
                        Key::Int(i) => PhpKey::Int(*i),
                        Key::Str(s) => PhpKey::Str(s.as_bytes().to_vec()),
                    };
                    Ok((key, from_hook_value(v)?))
                })
                .collect::<Result<_, ConvertError>>()?,
        ),
        Value::Opaque(any) => any
            .downcast_ref::<OpaquePhpValue>()
            .map(|o| o.0.clone())
            .ok_or(ConvertError::Foreign)?,
    })
}
