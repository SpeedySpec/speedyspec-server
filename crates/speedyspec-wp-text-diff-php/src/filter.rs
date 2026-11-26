//! `process_text_diff_html` through the Hook API.

use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_wp_hooks::WpHookApi;
use speedyspec_wp_hooks_contracts::{Count, HookError, HookName, HookValue};
use speedyspec_wp_text_diff::contracts::{
    CheckedLineFilter, LineHtmlFilter, RowKind, TextDifferError,
};

/// The `process_text_diff_html` filter, as an adapter over [`WpHookApi`].
///
/// Each row asks the Hook API once: with no callback the run is only counted (`did_filter()` stays WordPress's) and
/// PHP is not entered. A failing callback stops the filtering; [`CheckedLineFilter::take_failure`] then gives the
/// failure to rethrow.
pub struct HookLineFilter {
    hooks: Rc<WpHookApi>,
    name: HookName,
    failure: RefCell<Option<TextDifferError>>,
}

impl HookLineFilter {
    /// The filter over `hooks`.
    pub fn new(hooks: Rc<WpHookApi>) -> HookLineFilter {
        HookLineFilter {
            hooks,
            name: HookName::from("process_text_diff_html"),
            failure: RefCell::new(None),
        }
    }

    fn fail(&self, error: TextDifferError) {
        self.failure.borrow_mut().get_or_insert(error);
    }
}

fn php_string(bytes: &[u8]) -> HookValue {
    match std::str::from_utf8(bytes) {
        Ok(s) => HookValue::Str(s.to_owned()),
        Err(_) => HookValue::Bytes(bytes.to_vec()),
    }
}

/// A Hook API failure as the port's error: the PHP exception rethrown when there is one, and PHP unwinding stays
/// silent.
pub(crate) fn hook_error(err: HookError) -> TextDifferError {
    match &err.payload {
        Some(payload) => match payload.downcast_ref::<HookValue>() {
            Some(exception) => TextDifferError::Rethrow(exception.clone()),
            None => TextDifferError::Unwinding,
        },
        None => TextDifferError::Throw {
            class: "Error".into(),
            message: err.message,
        },
    }
}

/// What PHP's string interpolation makes of a filter's result, for the values Rust can convert alone.
fn to_bytes(value: &HookValue) -> Option<Vec<u8>> {
    match value {
        HookValue::Str(s) => Some(s.as_bytes().to_vec()),
        HookValue::Bytes(b) => Some(b.clone()),
        HookValue::Int(i) => Some(i.to_string().into_bytes()),
        HookValue::Bool(true) => Some(b"1".to_vec()),
        HookValue::Bool(false) | HookValue::Null => Some(Vec::new()),
        _ => None,
    }
}

impl LineHtmlFilter for HookLineFilter {
    fn filter(&self, out: &mut Vec<u8>, from: usize, raw: &[u8], kind: RowKind) {
        if self.failure.borrow().is_some() {
            return;
        }
        match self.hooks.skip_if_empty(&self.name, Count::Filters) {
            Ok(true) => return,
            Ok(false) => {}
            Err(e) => return self.fail(hook_error(e)),
        }
        let processed = php_string(&out[from..]);
        let args = vec![php_string(raw), HookValue::Str(kind.as_str().to_owned())];
        let filtered = match self.hooks.apply_filters(&self.name, processed, args) {
            Ok(v) => v,
            Err(e) => return self.fail(hook_error(e)),
        };
        let text = match to_bytes(&filtered.read()) {
            Some(b) => b,
            // Floats, arrays and objects: PHP converts them itself, with its own warnings and errors.
            None => {
                match speedyspec_php::call_catching(&HookValue::Str("strval".into()), &[filtered]) {
                    Ok(v) => to_bytes(&v.read()).unwrap_or_default(),
                    Err(failure) => {
                        return self.fail(match failure.exception {
                            Some(exception) => TextDifferError::Rethrow(exception),
                            None => TextDifferError::Unwinding,
                        });
                    }
                }
            }
        };
        out.truncate(from);
        out.extend_from_slice(&text);
    }
}

impl CheckedLineFilter for HookLineFilter {
    fn take_failure(&self) -> Option<TextDifferError> {
        self.failure.borrow_mut().take()
    }
}
