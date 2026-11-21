use std::fmt;
use std::rc::Rc;

use crate::Bytes;

/// A scalar or list as a `wp_kses_allowed_html` filter left it, kept so kses can compare it the way PHP 8 does.
#[derive(Clone, Debug, PartialEq)]
pub enum RuleValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(Bytes),
    List(Vec<RuleValue>),
}

/// A `value_callback`: true keeps the attribute.
///
/// `Rc`, not `Arc`: a PHP callable cannot leave its thread.
pub type ValueCallback = Rc<dyn Fn(&[u8]) -> Result<bool, crate::KsesError>>;

/// One check from an attribute's rule array, keyed as PHP keys it (`maxlen`, `values`, ...).
#[derive(Clone)]
pub enum AttrRule {
    MaxLen(RuleValue),
    MinLen(RuleValue),
    MaxVal(RuleValue),
    MinVal(RuleValue),
    Valueless(RuleValue),
    Values(Vec<RuleValue>),
    ValueCallback(ValueCallback),
    /// A key `wp_kses_check_attr_val` ignores, `required` among them, kept in order.
    Other(Bytes, RuleValue),
}

impl fmt::Debug for AttrRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AttrRule::MaxLen(v) => f.debug_tuple("MaxLen").field(v).finish(),
            AttrRule::MinLen(v) => f.debug_tuple("MinLen").field(v).finish(),
            AttrRule::MaxVal(v) => f.debug_tuple("MaxVal").field(v).finish(),
            AttrRule::MinVal(v) => f.debug_tuple("MinVal").field(v).finish(),
            AttrRule::Valueless(v) => f.debug_tuple("Valueless").field(v).finish(),
            AttrRule::Values(v) => f.debug_tuple("Values").field(v).finish(),
            AttrRule::ValueCallback(_) => f.write_str("ValueCallback(..)"),
            AttrRule::Other(k, v) => f
                .debug_tuple("Other")
                .field(&String::from_utf8_lossy(k))
                .field(v)
                .finish(),
        }
    }
}

impl PartialEq for AttrRule {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (AttrRule::MaxLen(a), AttrRule::MaxLen(b))
            | (AttrRule::MinLen(a), AttrRule::MinLen(b))
            | (AttrRule::MaxVal(a), AttrRule::MaxVal(b))
            | (AttrRule::MinVal(a), AttrRule::MinVal(b))
            | (AttrRule::Valueless(a), AttrRule::Valueless(b)) => a == b,
            (AttrRule::Values(a), AttrRule::Values(b)) => a == b,
            (AttrRule::ValueCallback(a), AttrRule::ValueCallback(b)) => Rc::ptr_eq(a, b),
            (AttrRule::Other(ka, a), AttrRule::Other(kb, b)) => ka == kb && a == b,
            _ => false,
        }
    }
}

/// What an allowlist says about one attribute name.
#[derive(Clone, Debug, PartialEq)]
pub enum AttrRules {
    /// PHP `''` or `null`: the attribute is removed.
    Denied,
    /// Any other non-array value: allowed with no checks.
    Allowed,
    /// An array: every rule must pass.
    Rules(Vec<AttrRule>),
}

impl AttrRules {
    /// Whether a tag without this attribute, or with it failing, is dropped whole
    /// (`isset( $rules['required'] ) && true === $rules['required']`).
    pub fn is_required(&self) -> bool {
        match self {
            AttrRules::Rules(rules) => rules.iter().any(
                |r| matches!(r, AttrRule::Other(k, RuleValue::Bool(true)) if &**k == b"required"),
            ),
            _ => false,
        }
    }
}
