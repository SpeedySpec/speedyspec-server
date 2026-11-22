//! The `Identity` port's PHP adapter: capability checks through WordPress's `user_can()`.
//!
//! A Rust adapter replaces it once users and capabilities are ported; the area takes any `Rc<dyn Identity>`.

use speedyspec_operation_contracts::{Identity, Value};
use speedyspec_php_ext::Value as PhpValue;

use crate::convert::to_php;

/// Asks a PHP function shaped like `user_can( $user, $capability, ...$args )`.
pub struct PhpIdentity {
    function: String,
}

impl PhpIdentity {
    /// Over `user_can`.
    pub fn new() -> Self {
        PhpIdentity::with_function("user_can")
    }

    /// Over the PHP function `name`, which takes `( int $user, string $capability, ...$args )`.
    pub fn with_function(name: &str) -> Self {
        PhpIdentity {
            function: name.to_owned(),
        }
    }
}

impl Default for PhpIdentity {
    fn default() -> Self {
        PhpIdentity::new()
    }
}

impl Identity for PhpIdentity {
    /// Anonymous callers are user 0, as in WordPress. A missing function, or a call that fails, denies.
    fn can(&self, user: Option<u64>, capability: &str, args: &[Value]) -> bool {
        let user = user.map_or(0, |u| i64::try_from(u).unwrap_or(0));
        let mut call_args = vec![PhpValue::Int(user), PhpValue::Str(capability.to_owned())];
        call_args.extend(args.iter().map(to_php));
        match speedyspec_php::call(&PhpValue::Str(self.function.clone()), &call_args) {
            Ok(result) => truthy(&result),
            Err(err) => {
                eprintln!(
                    "speedyspec_wordpress: the capability check {}() failed: {err}",
                    self.function
                );
                false
            }
        }
    }
}

/// PHP's truthiness for a returned value.
pub(crate) fn truthy(value: &PhpValue) -> bool {
    match value.read().as_ref() {
        PhpValue::Null => false,
        PhpValue::Bool(b) => *b,
        PhpValue::Int(i) => *i != 0,
        PhpValue::Float(f) => *f != 0.0,
        PhpValue::Str(s) => !(s.is_empty() || s == "0"),
        PhpValue::Bytes(b) => !(b.is_empty() || b == b"0"),
        PhpValue::Array(items) => !items.is_empty(),
        PhpValue::Opaque(_) => true,
    }
}
