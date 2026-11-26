//! The `wp_text_diff()` port over PHP (Adapters).

use std::rc::Rc;

use speedyspec_php::{CallFailure, PhpError};
use speedyspec_php_ext::PhpObject;
use speedyspec_wp_hooks::WpHookApi;
use speedyspec_wp_hooks_contracts::{HookValue as Value, Key};
use speedyspec_wp_text_diff::contracts::{
    CheckedLineFilter, RowLabels, TextDiffHost, TextDiffer, TextDifferError,
};

use crate::filter::HookLineFilter;

const HELPERS: &str = "SpeedySpec\\WpTextDiff\\";

/// A failed call as the port's error: the exception PHP threw is kept, and PHP unwinding stays silent.
fn call_error(failure: CallFailure) -> TextDifferError {
    match failure.exception {
        Some(exception) => TextDifferError::Rethrow(exception),
        None => match failure.error {
            PhpError::Exit(_) | PhpError::Fatal { .. } => TextDifferError::Unwinding,
            other => TextDifferError::Port(other.to_string()),
        },
    }
}

/// Calls `callable(...$args)`, keeping the exception PHP throws.
fn call(callable: &Value, args: &[Value]) -> Result<Value, TextDifferError> {
    speedyspec_php::call_catching(callable, args).map_err(call_error)
}

/// Calls one of the facade's helpers in `SpeedySpec\WpTextDiff`.
fn helper(name: &str, args: &[Value]) -> Result<Value, TextDifferError> {
    call(&Value::Str(format!("{HELPERS}{name}")), args)
}

/// A PHP string's bytes.
fn bytes(value: &Value) -> Option<Vec<u8>> {
    match &*value.read() {
        Value::Str(s) => Some(s.as_bytes().to_vec()),
        Value::Bytes(b) => Some(b.clone()),
        _ => None,
    }
}

/// What WordPress's `wp_text_diff()` reaches in PHP: the facade's helpers and the Hook API.
///
/// When the parsed arguments cannot reach Rust (a key that isn't UTF-8, a recursive or too deep array),
/// [`TextDiffHost::parse_args`] answers `null`, which no parse gives, so the call takes the fallback; the fallback then
/// renders with the array the facade kept, instead of parsing the arguments (and running `wp_parse_str`) twice.
pub struct PhpTextDiffHost {
    hooks: Rc<WpHookApi>,
}

impl PhpTextDiffHost {
    /// The host, filtering rows through `hooks`.
    pub fn new(hooks: Rc<WpHookApi>) -> PhpTextDiffHost {
        PhpTextDiffHost { hooks }
    }
}

impl TextDiffHost for PhpTextDiffHost {
    fn parse_args(&self, args: &Value) -> Result<Value, TextDifferError> {
        let callable = Value::Str(format!("{HELPERS}parse_args"));
        match speedyspec_php::call_catching(&callable, std::slice::from_ref(args)) {
            Err(failure)
                if failure.exception.is_none()
                    && matches!(failure.error, PhpError::Conversion(_)) =>
            {
                Ok(Value::Null)
            }
            other => other.map_err(call_error),
        }
    }

    fn load_renderer(&self) -> Result<(), TextDifferError> {
        helper("load_renderer", &[]).map(|_| ())
    }

    fn row_labels(&self) -> Result<RowLabels, TextDifferError> {
        let labels = helper("row_labels", &[])?;
        let read = labels.read();
        let label = |i: usize| -> Result<Vec<u8>, TextDifferError> {
            match &*read {
                Value::Array(items) => items.get(i).and_then(|(_, v)| bytes(v)),
                _ => None,
            }
            .ok_or_else(|| {
                TextDifferError::Port("row_labels() did not return three strings".into())
            })
        };
        Ok(RowLabels {
            added: label(0)?,
            deleted: label(1)?,
            unchanged: label(2)?,
        })
    }

    fn legacy(&self, left: &Value, right: &Value, args: &Value) -> Result<Value, TextDifferError> {
        match args {
            Value::Null => helper("legacy_with_kept_args", &[left.clone(), right.clone()]),
            _ => helper(
                "legacy_wp_text_diff",
                &[left.clone(), right.clone(), args.clone()],
            ),
        }
    }

    fn line_filter(&self) -> Box<dyn CheckedLineFilter> {
        Box::new(HookLineFilter::new(self.hooks.clone()))
    }
}

/// A PHP object implementing `SpeedySpec\WpTextDiff\TextDiffer`.
pub struct PhpReplacement(PhpObject);

impl PhpReplacement {
    pub fn new(object: PhpObject) -> PhpReplacement {
        PhpReplacement(object)
    }
}

impl TextDiffer for PhpReplacement {
    fn diff(&self, left: &Value, right: &Value, args: &Value) -> Result<Value, TextDifferError> {
        let callable = Value::Array(vec![
            (Key::Int(0), self.0.value().clone()),
            (Key::Int(1), Value::Str("diff".into())),
        ]);
        call(&callable, &[left.clone(), right.clone(), args.clone()])
    }
}
