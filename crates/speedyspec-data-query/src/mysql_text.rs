//! MySQL text for a syntax tree.

use std::ops::ControlFlow;

use sqlparser::ast::{Statement, Value, ValueWithSpan, VisitMut, VisitorMut};

/// The parser stores a string literal's value (MySQL's backslash escapes already applied), and the tree's `Display`
/// re-escapes only the quote. MySQL reads backslashes as escapes, so each literal is escaped again before the text
/// is written; otherwise `'a\\'` would reach the server as `'a\'` and end somewhere else.
struct Reescape;

fn mysql_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\0' => out.push_str("\\0"),
            '\u{1a}' => out.push_str("\\Z"),
            other => out.push(other),
        }
    }
    out
}

impl VisitorMut for Reescape {
    type Break = ();

    fn pre_visit_value(&mut self, value: &mut ValueWithSpan) -> ControlFlow<()> {
        if let Value::SingleQuotedString(s)
        | Value::DoubleQuotedString(s)
        | Value::NationalStringLiteral(s) = &mut value.value
        {
            *s = mysql_escape(s);
        }
        ControlFlow::Continue(())
    }
}

/// The statement as MySQL text, string literals escaped as MySQL reads them; placeholders stay `?`.
pub fn mysql_text(tree: &Statement) -> String {
    let mut tree = tree.clone();
    let _ = tree.visit(&mut Reescape);
    tree.to_string()
}
