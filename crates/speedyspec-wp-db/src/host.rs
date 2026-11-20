//! The host port (`WpdbHost`, Strategy): what only PHP's WordPress can do for `wpdb`.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::{DoingItWrong, WpValue};

/// `_doing_it_wrong`, `_deprecated_function`, `__()`, `defined`/`constant`, `is_multisite()`, `error_log()`, output,
/// `wp_die()`, `dead_db()`, `$EZSQL_ERROR[]` and `wp_debug_backtrace_summary('wpdb')`.
pub trait WpdbHost {
    fn doing_it_wrong(&self, notice: &DoingItWrong);
    fn translate(&self, text: &str) -> String;
    fn constant(&self, name: &str) -> Option<WpValue>;
    fn is_multisite(&self) -> bool;
    fn error_log(&self, message: &str, destination: Option<&str>);
    fn output(&self, html: &str);
    fn die(&self, message: &str);
    fn dead_db(&self);
    fn record_error(&self, query: &str, error: &str);
    fn caller(&self) -> String;
    fn deprecated_function(&self, function: &str, version: &str, replacement: &str);
}

/// One call a `MemoryWpdbHost` received.
#[derive(Debug, Clone, PartialEq)]
pub enum HostCall {
    DoingItWrong(DoingItWrong),
    ErrorLog(String, Option<String>),
    Output(String),
    Die(String),
    DeadDb,
    RecordError(String, String),
    DeprecatedFunction(String, String, String),
}

/// A host that records every call (for tests and hosts without WordPress).
#[derive(Default)]
pub struct MemoryWpdbHost {
    pub calls: RefCell<Vec<HostCall>>,
    pub constants: RefCell<HashMap<String, WpValue>>,
    pub multisite: RefCell<bool>,
}

impl MemoryWpdbHost {
    pub fn new() -> MemoryWpdbHost {
        MemoryWpdbHost::default()
    }

    pub fn define(&self, name: &str, value: WpValue) {
        self.constants.borrow_mut().insert(name.to_string(), value);
    }

    pub fn calls(&self) -> Vec<HostCall> {
        self.calls.borrow().clone()
    }

    fn push(&self, call: HostCall) {
        self.calls.borrow_mut().push(call);
    }
}

impl WpdbHost for MemoryWpdbHost {
    fn doing_it_wrong(&self, notice: &DoingItWrong) {
        self.push(HostCall::DoingItWrong(notice.clone()));
    }
    fn translate(&self, text: &str) -> String {
        text.to_string()
    }
    fn constant(&self, name: &str) -> Option<WpValue> {
        self.constants.borrow().get(name).cloned()
    }
    fn is_multisite(&self) -> bool {
        *self.multisite.borrow()
    }
    fn error_log(&self, message: &str, destination: Option<&str>) {
        self.push(HostCall::ErrorLog(
            message.into(),
            destination.map(str::to_string),
        ));
    }
    fn output(&self, html: &str) {
        self.push(HostCall::Output(html.into()));
    }
    fn die(&self, message: &str) {
        self.push(HostCall::Die(message.into()));
    }
    fn dead_db(&self) {
        self.push(HostCall::DeadDb);
    }
    fn record_error(&self, query: &str, error: &str) {
        self.push(HostCall::RecordError(query.into(), error.into()));
    }
    fn caller(&self) -> String {
        String::new()
    }
    fn deprecated_function(&self, function: &str, version: &str, replacement: &str) {
        self.push(HostCall::DeprecatedFunction(
            function.into(),
            version.into(),
            replacement.into(),
        ));
    }
}
