//! `wp_text_diff()` as WordPress implements it, behind the [`TextDiffer`] port (Adapter over [`TextDiffHost`]).

use speedyspec_hooks::Key;
use speedyspec_wp_text_diff_contracts::{
    TableOptions, TextDiffHost, TextDiffer, TextDifferError, Title, Value,
};

use crate::api::{TextDiff, TextDiffError};
use crate::horde::HordeSequenceDiffer;
use crate::whitespace::WordPressWhitespace;

/// The keys the native table covers; WordPress's renderer looks at any other.
const NATIVE_KEYS: [&str; 4] = ["title", "title_left", "title_right", "show_split_view"];

/// PHP's truth, as `! empty()` sees it: `null`, `false`, `0`, `0.0`, `""`, `"0"` and `[]` are false.
pub fn php_truthy(value: &Value) -> bool {
    match &*value.read() {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Int(n) => *n != 0,
        Value::Float(f) => *f != 0.0,
        Value::Str(s) => !(s.is_empty() || s == "0"),
        Value::Bytes(b) => !(b.is_empty() || b == b"0"),
        Value::Array(a) => !a.is_empty(),
        Value::Opaque(_) => true,
    }
}

/// A PHP string's bytes, or `None` for any other value (`is_string()`).
fn string_bytes(value: &Value) -> Option<Vec<u8>> {
    match &*value.read() {
        Value::Str(s) => Some(s.as_bytes().to_vec()),
        Value::Bytes(b) => Some(b.clone()),
        _ => None,
    }
}

/// The table options when the native table renders `args` exactly as WordPress would: three string titles and no
/// key but the four it covers.
fn native_options(args: &Value) -> Option<TableOptions> {
    let read = args.read();
    let Value::Array(items) = &*read else {
        return None;
    };
    let mut titles: [Option<Vec<u8>>; 3] = [None, None, None];
    let mut split_view = false;
    for (key, value) in items {
        let Key::Str(key) = key else {
            return None;
        };
        match NATIVE_KEYS.iter().position(|k| k == key)? {
            3 => split_view = php_truthy(value),
            i => titles[i] = Some(string_bytes(value)?),
        }
    }
    let [title, title_left, title_right] = titles;
    Some(TableOptions {
        title: Title::new(title?),
        title_left: Title::new(title_left?),
        title_right: Title::new(title_right?),
        split_view,
    })
}

/// An engine failure as the exception WordPress throws there.
fn engine_error(err: TextDiffError) -> TextDifferError {
    let (class, message) = match err {
        TextDiffError::InvalidUtf8 => (
            "TypeError",
            "wp_text_diff(): a changed line is not valid UTF-8, so it cannot be split into words"
                .to_owned(),
        ),
        TextDiffError::DivisionByZero => ("DivisionByZeroError", "Division by zero".to_owned()),
        e @ TextDiffError::TooLarge { .. } => ("Error", format!("wp_text_diff(): {e}")),
    };
    TextDifferError::Throw {
        class: class.into(),
        message,
    }
}

/// Bytes as a PHP string value.
fn php_string(bytes: Vec<u8>) -> Value {
    match String::from_utf8(bytes) {
        Ok(s) => Value::Str(s),
        Err(e) => Value::Bytes(e.into_bytes()),
    }
}

/// WordPress's `wp_text_diff()`: the arguments parsed and the table rendered in Rust, byte for byte as WordPress
/// renders it, with what is PHP's reached through `H`.
///
/// Calls the native table doesn't cover (texts or titles that aren't strings, renderer properties passed as
/// arguments) go to [`TextDiffHost::legacy`], so nothing changes for them. The row labels are translated once per
/// call rather than once per row.
pub struct WordPressTextDiffer<H> {
    host: H,
    diff: TextDiff<HordeSequenceDiffer, WordPressWhitespace>,
}

impl<H: TextDiffHost> WordPressTextDiffer<H> {
    /// WordPress's `wp_text_diff()` over `host`.
    pub fn new(host: H) -> WordPressTextDiffer<H> {
        WordPressTextDiffer {
            host,
            diff: TextDiff::new(HordeSequenceDiffer, WordPressWhitespace),
        }
    }
}

impl<H: TextDiffHost> TextDiffer for WordPressTextDiffer<H> {
    fn diff(&self, left: &Value, right: &Value, args: &Value) -> Result<Value, TextDifferError> {
        let args = self.host.parse_args(args)?;
        // As WordPress does: the classes are defined after the first call, for code that relies on it.
        self.host.load_renderer()?;
        let (Some(left_bytes), Some(right_bytes), Some(options)) = (
            string_bytes(left),
            string_bytes(right),
            native_options(&args),
        ) else {
            return self.host.legacy(left, right, &args);
        };
        let labels = self.host.row_labels()?;
        let filter = self.host.line_filter();
        let table = self
            .diff
            .table(&left_bytes, &right_bytes, &options, &labels, &*filter);
        if let Some(failure) = filter.take_failure() {
            return Err(failure);
        }
        match table.map_err(engine_error)? {
            Some(html) => Ok(php_string(html)),
            None => Ok(Value::Str(String::new())),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use speedyspec_hooks::Key;
    use speedyspec_wp_text_diff_contracts::{
        CheckedLineFilter, LineHtmlFilter, RowKind, RowLabels, TextDiffHost, TextDiffer,
        TextDifferError, Value,
    };

    use super::{WordPressTextDiffer, php_truthy};

    /// A host that records each call and answers as WordPress would for array arguments.
    #[derive(Default)]
    struct FakeHost {
        log: Rc<RefCell<Vec<&'static str>>>,
        fail_parse: bool,
        fail_filter: bool,
    }

    struct FakeFilter {
        fail: bool,
        failure: RefCell<Option<TextDifferError>>,
    }

    impl LineHtmlFilter for FakeFilter {
        fn filter(&self, _out: &mut Vec<u8>, _from: usize, _raw: &[u8], _kind: RowKind) {
            if self.fail {
                self.failure
                    .borrow_mut()
                    .get_or_insert(TextDifferError::Rethrow(Value::Str("thrown".into())));
            }
        }
    }

    impl CheckedLineFilter for FakeFilter {
        fn take_failure(&self) -> Option<TextDifferError> {
            self.failure.borrow_mut().take()
        }
    }

    impl TextDiffHost for FakeHost {
        fn parse_args(&self, args: &Value) -> Result<Value, TextDifferError> {
            self.log.borrow_mut().push("parse_args");
            if self.fail_parse {
                return Err(TextDifferError::Rethrow(Value::Str("parse".into())));
            }
            let mut parsed = vec![
                (Key::Str("title".into()), Value::Str(String::new())),
                (Key::Str("title_left".into()), Value::Str(String::new())),
                (Key::Str("title_right".into()), Value::Str(String::new())),
                (Key::Str("show_split_view".into()), Value::Bool(true)),
            ];
            if let Value::Array(given) = args {
                for (key, value) in given {
                    match parsed.iter_mut().find(|(k, _)| k == key) {
                        Some(slot) => slot.1 = value.clone(),
                        None => parsed.push((key.clone(), value.clone())),
                    }
                }
            }
            Ok(Value::Array(parsed))
        }

        fn load_renderer(&self) -> Result<(), TextDifferError> {
            self.log.borrow_mut().push("load_renderer");
            Ok(())
        }

        fn row_labels(&self) -> Result<RowLabels, TextDifferError> {
            self.log.borrow_mut().push("row_labels");
            Ok(RowLabels::default())
        }

        fn legacy(
            &self,
            _left: &Value,
            _right: &Value,
            _args: &Value,
        ) -> Result<Value, TextDifferError> {
            self.log.borrow_mut().push("legacy");
            Ok(Value::Str("legacy".into()))
        }

        fn line_filter(&self) -> Box<dyn CheckedLineFilter> {
            self.log.borrow_mut().push("line_filter");
            Box::new(FakeFilter {
                fail: self.fail_filter,
                failure: RefCell::new(None),
            })
        }
    }

    fn s(text: &str) -> Value {
        Value::Str(text.into())
    }

    fn args(pairs: &[(&str, Value)]) -> Value {
        Value::Array(
            pairs
                .iter()
                .map(|(k, v)| (Key::Str((*k).into()), v.clone()))
                .collect(),
        )
    }

    fn text(value: &Value) -> String {
        match value {
            Value::Str(s) => s.clone(),
            other => panic!("not a string: {other:?}"),
        }
    }

    #[test]
    fn native_args_render_in_rust() {
        let host = FakeHost::default();
        let log = host.log.clone();
        let out = WordPressTextDiffer::new(host)
            .diff(&s("a"), &s("b"), &Value::Null)
            .unwrap();
        assert!(text(&out).starts_with("<table class='diff is-split-view'>"));
        assert!(!log.borrow().contains(&"legacy"));
    }

    #[test]
    fn steps_run_in_wordpress_order() {
        let host = FakeHost::default();
        let log = host.log.clone();
        WordPressTextDiffer::new(host)
            .diff(&s("a"), &s("b"), &Value::Null)
            .unwrap();
        assert_eq!(
            *log.borrow(),
            ["parse_args", "load_renderer", "row_labels", "line_filter"]
        );
    }

    #[test]
    fn extra_key_goes_to_legacy() {
        let host = FakeHost::default();
        let out = WordPressTextDiffer::new(host)
            .diff(
                &s("a"),
                &s("b"),
                &args(&[("diff_threshold", Value::Float(0.1))]),
            )
            .unwrap();
        assert_eq!(text(&out), "legacy");
    }

    #[test]
    fn non_string_title_goes_to_legacy() {
        let host = FakeHost::default();
        let out = WordPressTextDiffer::new(host)
            .diff(&s("a"), &s("b"), &args(&[("title", Value::Int(5))]))
            .unwrap();
        assert_eq!(text(&out), "legacy");
    }

    #[test]
    fn non_string_text_goes_to_legacy() {
        let host = FakeHost::default();
        let log = host.log.clone();
        let out = WordPressTextDiffer::new(host)
            .diff(&Value::Int(1), &s("b"), &Value::Null)
            .unwrap();
        assert_eq!(text(&out), "legacy");
        assert_eq!(*log.borrow(), ["parse_args", "load_renderer", "legacy"]);
    }

    #[test]
    fn split_view_follows_empty() {
        for (value, split) in [
            (s("0"), false),
            (Value::Int(0), false),
            (Value::Array(Vec::new()), false),
            (Value::Null, false),
            (s("00"), true),
            (Value::Int(1), true),
        ] {
            let out = WordPressTextDiffer::new(FakeHost::default())
                .diff(
                    &s("a"),
                    &s("b"),
                    &args(&[("show_split_view", value.clone())]),
                )
                .unwrap();
            assert_eq!(
                text(&out).contains("is-split-view"),
                split,
                "show_split_view = {value:?}"
            );
        }
    }

    #[test]
    fn titles_are_printed() {
        let out = WordPressTextDiffer::new(FakeHost::default())
            .diff(&s("a"), &s("b"), &args(&[("title", s("T"))]))
            .unwrap();
        assert!(text(&out).contains("<caption class='diff-title'>T</caption>"));
    }

    #[test]
    fn equal_texts_are_empty_string() {
        let out = WordPressTextDiffer::new(FakeHost::default())
            .diff(&s("same"), &s("same"), &Value::Null)
            .unwrap();
        assert_eq!(text(&out), "");
    }

    #[test]
    fn filter_failure_is_returned() {
        let host = FakeHost {
            fail_filter: true,
            ..FakeHost::default()
        };
        let err = WordPressTextDiffer::new(host)
            .diff(&s("a\nsame"), &s("b\nsame"), &Value::Null)
            .unwrap_err();
        assert!(matches!(err, TextDifferError::Rethrow(Value::Str(ref t)) if t == "thrown"));
    }

    #[test]
    fn host_failure_propagates() {
        let host = FakeHost {
            fail_parse: true,
            ..FakeHost::default()
        };
        let err = WordPressTextDiffer::new(host)
            .diff(&s("a"), &s("b"), &Value::Null)
            .unwrap_err();
        assert!(matches!(err, TextDifferError::Rethrow(Value::Str(ref t)) if t == "parse"));
    }

    #[test]
    fn truthiness_is_php_empty() {
        assert!(!php_truthy(&s("")));
        assert!(!php_truthy(&Value::Float(0.0)));
        assert!(php_truthy(&s(" ")));
        assert!(php_truthy(&Value::Array(vec![(Key::Int(0), Value::Null)])));
    }
}
