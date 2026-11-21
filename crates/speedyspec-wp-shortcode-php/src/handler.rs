//! Handlers as PHP sees them: PHP callables, and Rust handlers declared as native functions.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use speedyspec_hooks::{Key, Value};
use speedyspec_php::{CallFailure, Callable, PhpError};
use speedyspec_wp_shortcode_contracts::{
    AttKey, ParsedAtts, PhpValue, ShortcodeError, ShortcodeHandler, Thrown, php_bytes,
};

/// A PHP failure as a shortcode error.
///
/// - An exception keeps its object, so it is rethrown unchanged.
/// - PHP unwinding (a fatal error, `exit()`, a destroyed fiber) carries `null`, so nothing is thrown.
/// - Anything else (a result that cannot reach Rust, a refused call) is a plain error, thrown as `Error`: never a
///   silent unwinding, which would lose the output without a word.
pub(crate) fn thrown(failure: CallFailure) -> ShortcodeError {
    let message = failure.error.to_string();
    match (failure.exception, &failure.error) {
        (Some(exception), _) => ShortcodeError::Thrown(Thrown { message, payload: Rc::new(exception) }),
        (None, PhpError::Fatal { .. } | PhpError::Exit(_)) => {
            ShortcodeError::Thrown(Thrown { message, payload: Rc::new(Value::Null) })
        }
        (None, PhpError::Exception { class, .. }) if class == "GracefulExit" => {
            ShortcodeError::Thrown(Thrown { message, payload: Rc::new(Value::Null) })
        }
        (None, _) => ShortcodeError::Handler(message),
    }
}

/// A PHP callable stored in `$shortcode_tags`, as a handler (an Adapter).
///
/// It is called through Zend's fast path when it resolves, and through `call_user_func_array()` otherwise, as
/// WordPress calls it.
pub struct PhpShortcodeHandler {
    callable: Callable,
}

impl PhpShortcodeHandler {
    /// Wraps the stored value.
    pub fn new(value: Value) -> Self {
        Self { callable: Callable::new(value) }
    }
}

impl ShortcodeHandler for PhpShortcodeHandler {
    fn call(&self, atts: &ParsedAtts, content: Option<&[u8]>, tag: &[u8]) -> Result<PhpValue, ShortcodeError> {
        let content = content.map_or(Value::Null, |c| php_bytes(c.to_vec()));
        self.callable.call(&[atts.to_value(), content, php_bytes(tag.to_vec())]).map_err(thrown)
    }

    fn is_callable(&self) -> bool {
        matches!(
            speedyspec_php::call(&Value::Str("is_callable".into()), std::slice::from_ref(self.callable.value())),
            Ok(Value::Bool(true))
        )
    }

    fn php_callable(&self) -> Option<PhpValue> {
        Some(self.callable.value().clone())
    }
}

/// A Rust handler under the name of the native PHP function it is declared as.
pub(crate) struct NamedRustHandler {
    pub name: String,
    pub inner: Rc<dyn ShortcodeHandler>,
}

impl ShortcodeHandler for NamedRustHandler {
    fn call(&self, atts: &ParsedAtts, content: Option<&[u8]>, tag: &[u8]) -> Result<PhpValue, ShortcodeError> {
        self.inner.call(atts, content, tag)
    }

    fn php_callable(&self) -> Option<PhpValue> {
        Some(Value::Str(self.name.clone()))
    }
}

/// The Rust handlers, by their PHP function name in lower case (PHP function names are case-insensitive).
#[derive(Default)]
pub(crate) struct RustHandlers(pub RefCell<HashMap<String, Rc<NamedRustHandler>>>);

impl RustHandlers {
    pub(crate) fn find(&self, name: &str) -> Option<Rc<NamedRustHandler>> {
        self.0.borrow().get(&name.to_ascii_lowercase()).cloned()
    }
}

/// A PHP array of attributes as [`ParsedAtts`], each value by PHP's string conversion.
pub(crate) fn atts_from_value(value: &Value, strings: &dyn speedyspec_wp_shortcode_contracts::PhpStrings) -> ParsedAtts {
    let mut atts = ParsedAtts::new();
    if let Value::Array(entries) = value {
        for (k, v) in entries {
            let key = match k {
                Key::Int(i) => AttKey::Int(*i),
                Key::Str(s) => AttKey::Str(s.clone().into_bytes()),
            };
            atts.insert(key, strings.to_php_string(v).unwrap_or_default());
        }
    }
    atts
}
