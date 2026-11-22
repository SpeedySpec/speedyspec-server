//! The `wp_text_diff()` port over PHP (Adapters).

use std::rc::Rc;

use speedyspec_php::PhpError;
use speedyspec_php_ext::PhpObject;
use speedyspec_wp_hooks::{HookApi, HookValue as Value, Key};
use speedyspec_wp_text_diff::contracts::{
    CheckedLineFilter, RowLabels, TextDiffHost, TextDiffer, TextDifferError,
};

use crate::filter::HookLineFilter;

const HELPERS: &str = "SpeedySpec\\WpTextDiff\\";

/// Calls `callable(...$args)`, keeping the exception PHP throws.
fn call(callable: &Value, args: &[Value]) -> Result<Value, TextDifferError> {
    speedyspec_php::call_catching(callable, args).map_err(|failure| match failure.exception {
        Some(exception) => TextDifferError::Rethrow(exception),
        None => match failure.error {
            PhpError::Exit(_) | PhpError::Fatal { .. } => TextDifferError::Unwinding,
            other => TextDifferError::Port(other.to_string()),
        },
    })
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
pub struct PhpTextDiffHost {
    hooks: Rc<HookApi>,
}

impl PhpTextDiffHost {
    /// The host, filtering rows through `hooks`.
    pub fn new(hooks: Rc<HookApi>) -> PhpTextDiffHost {
        PhpTextDiffHost { hooks }
    }
}

impl TextDiffHost for PhpTextDiffHost {
    fn parse_args(&self, args: &Value) -> Result<Value, TextDifferError> {
        helper("parse_args", std::slice::from_ref(args))
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
        helper(
            "legacy_wp_text_diff",
            &[left.clone(), right.clone(), args.clone()],
        )
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
