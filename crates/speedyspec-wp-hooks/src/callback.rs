//! Callbacks and their identity.

use std::any::Any;
use std::fmt;
use std::rc::Rc;

use crate::HookValue;

/// A failed hook call: a callback failed (a PHP callback threw), or an argument cannot be used (a PHP `TypeError`).
#[derive(Clone)]
pub struct HookError {
    pub message: String,
    /// What a language bridge needs to rethrow the original failure (a PHP exception object).
    pub payload: Option<Rc<dyn Any>>,
}

impl HookError {
    pub fn message(message: impl Into<String>) -> HookError {
        HookError {
            message: message.into(),
            payload: None,
        }
    }
}

impl fmt::Debug for HookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HookError({:?})", self.message)
    }
}

impl fmt::Display for HookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for HookError {}

/// Runs a callback with the arguments WordPress passes it.
pub trait HookCallback {
    fn call(&self, args: &[HookValue]) -> Result<HookValue, HookError>;
}

impl<F: Fn(&[HookValue]) -> Result<HookValue, HookError>> HookCallback for F {
    fn call(&self, args: &[HookValue]) -> Result<HookValue, HookError> {
        self(args)
    }
}

/// A registered callback.
#[derive(Clone)]
pub enum Callback {
    /// A Rust callback with the id it is known by (its "function name").
    Rust {
        id: String,
        run: Rc<dyn HookCallback>,
    },
    /// A callback from another language: its own value (a PHP callable, shown in `$wp_filter`), the id
    /// [`CallbackIdentity`] gives it (`None` when WordPress would not register it), and how to run it.
    Foreign {
        value: HookValue,
        id: Option<String>,
        run: Rc<dyn HookCallback>,
    },
}

impl Callback {
    pub fn rust(id: &str, run: impl HookCallback + 'static) -> Callback {
        Callback::Rust {
            id: id.to_string(),
            run: Rc::new(run),
        }
    }

    pub fn id(&self) -> Option<&str> {
        match self {
            Callback::Rust { id, .. } => Some(id),
            Callback::Foreign { id, .. } => id.as_deref(),
        }
    }

    /// Runs the callback. A foreign callback gets the arguments as they came (values its language passed through
    /// unconverted stay so); a Rust callback reads them ([`HookValue::read`]), so only a Rust callback converts.
    pub fn call(&self, args: &[HookValue]) -> Result<HookValue, HookError> {
        match self {
            Callback::Foreign { run, .. } => run.call(args),
            Callback::Rust { run, .. } => {
                if args.iter().any(HookValue::needs_reading) {
                    let read: Vec<HookValue> = args.iter().map(|v| v.read().into_owned()).collect();
                    run.call(&read)
                } else {
                    run.call(args)
                }
            }
        }
    }

    /// The value `$wp_filter` shows as the callback's `function`.
    pub fn value(&self) -> HookValue {
        match self {
            Callback::Rust { id, .. } => HookValue::Str(id.clone()),
            Callback::Foreign { value, .. } => value.clone(),
        }
    }
}

impl fmt::Debug for Callback {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Callback({:?})", self.id())
    }
}

/// A callback's id, as `remove_filter()` and `has_filter()` look it up; `None` for a callback WordPress cannot
/// identify (such a callback is never found).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallbackId(pub Option<String>);

/// The shape of a callable, as `_wp_filter_build_unique_id()` sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallbackShape {
    /// A string: a function name or `"Class::method"`.
    Name(String),
    /// A closure or invokable object, by its object id.
    Object(u64),
    /// `[$object, 'method']`.
    Method { object: u64, method: String },
    /// `['Class', 'method']`.
    Static { class: String, method: String },
    /// Anything else.
    Invalid,
}

/// WordPress's `_wp_filter_build_unique_id()`.
pub struct CallbackIdentity;

impl CallbackIdentity {
    pub fn unique_id(shape: &CallbackShape) -> Option<String> {
        match shape {
            CallbackShape::Name(name) => Some(name.clone()),
            CallbackShape::Object(id) => Some(format!("spl_object_id:{id}")),
            CallbackShape::Method { object, method } => Some(format!("{object}{method}")),
            CallbackShape::Static { class, method } => Some(format!("{class}::{method}")),
            CallbackShape::Invalid => None,
        }
    }
}
