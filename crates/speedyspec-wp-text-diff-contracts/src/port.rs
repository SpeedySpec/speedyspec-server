//! `wp_text_diff()` as a container port, and the ports its WordPress implementation reaches.
//!
//! WordPress makes `wp_text_diff()` pluggable: a plugin replaces it by declaring it before `pluggable.php` does.
//! SpeedySpec replaces that mechanism with the service container. [`TextDiffer`] is the function, bound under
//! [`TEXT_DIFFER`]; anything bound there answers every call, from PHP and from Rust.

use crate::{LineHtmlFilter, RowLabels};

pub use speedyspec_hooks::Value;

/// The container key of [`TextDiffer`]; also the name of its PHP twin interface.
pub const TEXT_DIFFER: &str = "SpeedySpec\\WpTextDiff\\TextDiffer";

/// Why `wp_text_diff()` didn't return.
#[derive(Clone, Debug)]
pub enum TextDifferError {
    /// What PHP throws: a `TypeError`, a `DivisionByZeroError` or an `Error`, with PHP's message.
    Throw {
        /// The exception's class.
        class: String,
        /// The exception's message.
        message: String,
    },
    /// A PHP exception object (a `Value::Opaque`) a filter callback, WordPress's classes or a replacement threw, to
    /// rethrow unchanged.
    Rethrow(Value),
    /// PHP is unwinding (`exit()` or a fatal error in a callback): nothing more runs, nothing is thrown.
    Unwinding,
    /// A replacement bound in the container failed in a way that isn't a PHP exception.
    Port(String),
}

impl std::fmt::Display for TextDifferError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TextDifferError::Throw { class, message } => write!(f, "{class}: {message}"),
            TextDifferError::Rethrow(_) => f.write_str("a PHP exception"),
            TextDifferError::Unwinding => f.write_str("PHP is unwinding"),
            TextDifferError::Port(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for TextDifferError {}

/// `wp_text_diff()`.
pub trait TextDiffer {
    /// Renders the diff of the "old" (`left`) and "new" (`right`) texts, all three arguments as the caller passed
    /// them (`args` is `null`, a query string or an array).
    ///
    /// Returns an empty string when the texts are equivalent, otherwise the HTML table, or whatever a replacement
    /// returns.
    fn diff(&self, left: &Value, right: &Value, args: &Value) -> Result<Value, TextDifferError>;
}

/// A `process_text_diff_html` filter that remembers why it stopped.
pub trait CheckedLineFilter: LineHtmlFilter {
    /// The failure that stopped the filtering, if any.
    fn take_failure(&self) -> Option<TextDifferError>;
}

/// What WordPress's `wp_text_diff()` reaches outside the diff itself: PHP's own functions and classes.
pub trait TextDiffHost {
    /// `wp_parse_args( $args, $defaults )` with `wp_text_diff()`'s defaults.
    fn parse_args(&self, args: &Value) -> Result<Value, TextDifferError>;

    /// Loads `wp-diff.php` when its classes aren't defined yet, as WordPress does on every call.
    fn load_renderer(&self) -> Result<(), TextDifferError>;

    /// The hidden row labels, translated.
    fn row_labels(&self) -> Result<RowLabels, TextDifferError>;

    /// Renders with WordPress's own classes, for the calls the native table doesn't cover; `args` is what
    /// [`TextDiffHost::parse_args`] returned.
    fn legacy(&self, left: &Value, right: &Value, args: &Value) -> Result<Value, TextDifferError>;

    /// A fresh `process_text_diff_html` filter for one call.
    fn line_filter(&self) -> Box<dyn CheckedLineFilter>;
}
