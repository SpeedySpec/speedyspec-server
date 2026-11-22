//! WordPress's `wp_text_diff()` as a native PHP function, over the Rust text diff (Facade, Adapter).
//!
//! [`TextDiffArea`] declares `wp_text_diff()` at the start of every request (from `text-diff.php`), inside
//! WordPress's own `function_exists` guard, so a plugin's replacement still wins. The PHP function parses the
//! arguments as WordPress does, then calls `SpeedySpec\WpTextDiff\Native\wp_text_diff`, which normalizes the
//! whitespace ([`WordPressWhitespace`]) and renders the table in Rust. Calls the native table doesn't cover (texts or
//! titles that aren't strings, renderer properties passed as arguments) go to WordPress's own classes instead, so
//! nothing changes for them.
//!
//! `process_text_diff_html` runs through the Hook API ([`HookLineFilter`]): with no callback it is only counted,
//! as WordPress counts it, and PHP is not entered.

mod filter;

use std::rc::Rc;

use speedyspec_php::NativeArea;
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_php::native::{self, NativeError};
use speedyspec_wp_hooks::{HookApi, HookValue};
use speedyspec_wp_text_diff::contracts::{RowLabels, TableOptions, Title};
use speedyspec_wp_text_diff::{HordeSequenceDiffer, TextDiff, TextDiffError, WordPressWhitespace};

pub use filter::HookLineFilter;

const FACADE: &str = include_str!("text-diff.php");

/// The native area of `wp_text_diff()`.
pub struct TextDiffArea {
    hooks: Rc<HookApi>,
}

impl TextDiffArea {
    /// The area, filtering rows through `hooks`.
    pub fn new(hooks: Rc<HookApi>) -> TextDiffArea {
        TextDiffArea { hooks }
    }
}

impl NativeArea for TextDiffArea {
    fn functions(&self) -> Vec<FunctionBuilder<'static>> {
        let hooks = self.hooks.clone();
        let diff = Rc::new(TextDiff::new(HordeSequenceDiffer, WordPressWhitespace));
        vec![native::function(
            "SpeedySpec\\WpTextDiff\\Native\\wp_text_diff",
            move |args| native_wp_text_diff(&diff, &hooks, args),
        )]
    }

    fn request_start(&self) {
        if let Err(err) = speedyspec_php::eval("speedyspec://wp-text-diff", FACADE) {
            eprintln!("speedyspec_wordpress: installing wp_text_diff() failed: {err}");
        }
    }
}

fn bytes(args: &[HookValue], i: usize) -> Result<Vec<u8>, NativeError> {
    match args.get(i) {
        Some(HookValue::Str(s)) => Ok(s.as_bytes().to_vec()),
        Some(HookValue::Bytes(b)) => Ok(b.clone()),
        other => Err(NativeError::throw(
            "TypeError",
            format!(
                "SpeedySpec\\WpTextDiff\\Native\\wp_text_diff(): Argument #{} must be of type string, {other:?} given",
                i + 1
            ),
        )),
    }
}

/// `(left, right, title, title_left, title_right, split_view, added, deleted, unchanged)`, all strings but
/// `split_view`; the facade has translated the labels, and the texts are normalized here.
fn native_wp_text_diff(
    diff: &TextDiff<HordeSequenceDiffer, WordPressWhitespace>,
    hooks: &Rc<HookApi>,
    args: &[HookValue],
) -> Result<HookValue, NativeError> {
    let options = TableOptions {
        title: Title::new(bytes(args, 2)?),
        title_left: Title::new(bytes(args, 3)?),
        title_right: Title::new(bytes(args, 4)?),
        split_view: matches!(args.get(5), Some(HookValue::Bool(true))),
    };
    let labels = RowLabels {
        added: bytes(args, 6)?,
        deleted: bytes(args, 7)?,
        unchanged: bytes(args, 8)?,
    };
    let filter = HookLineFilter::new(hooks.clone());
    let table = diff.table(
        &bytes(args, 0)?,
        &bytes(args, 1)?,
        &options,
        &labels,
        &filter,
    );
    if let Some(failure) = filter.take_failure() {
        return Err(failure);
    }
    match table {
        Ok(Some(html)) => Ok(match String::from_utf8(html) {
            Ok(s) => HookValue::Str(s),
            Err(e) => HookValue::Bytes(e.into_bytes()),
        }),
        Ok(None) => Ok(HookValue::Str(String::new())),
        Err(TextDiffError::InvalidUtf8) => Err(NativeError::throw(
            "TypeError",
            "wp_text_diff(): a changed line is not valid UTF-8, so it cannot be split into words",
        )),
        Err(TextDiffError::DivisionByZero) => Err(NativeError::throw(
            "DivisionByZeroError",
            "Division by zero",
        )),
        Err(e @ TextDiffError::TooLarge { .. }) => {
            Err(NativeError::throw("Error", format!("wp_text_diff(): {e}")))
        }
    }
}
