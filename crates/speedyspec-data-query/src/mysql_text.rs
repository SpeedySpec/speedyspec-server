//! MySQL text for a syntax tree.

use std::ops::ControlFlow;

use sqlparser::ast::{Statement, Value, ValueWithSpan, VisitMut, VisitorMut};

/// The parser stores a string literal's value (MySQL's backslash escapes already applied). Every literal is escaped
/// again before the text is written, as `mysqli_real_escape_string` does: backslashes doubled, and every quote of
/// either kind given a backslash.
///
/// The quotes matter. The tree's `Display` doubles a quote only when it guesses the quote isn't escaped yet: it skips
/// a quote that follows a backslash and a pair of quotes. With backslashes doubled but quotes left to it, the value
/// `x\' OR 1=1 -- ` printed as `'x\\' OR 1=1 -- '`, which MySQL ends after `x\` and runs the rest as SQL. Escaping
/// every quote here leaves `Display` nothing to guess.
struct Reescape;

fn mysql_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '"' => out.push_str("\\\""),
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

#[cfg(test)]
mod tests {
    use super::mysql_text;
    use sqlparser::ast::{Statement, Value, ValueWithSpan, VisitMut, VisitorMut};
    use sqlparser::dialect::MySqlDialect;
    use sqlparser::parser::Parser;
    use std::ops::ControlFlow;

    /// A statement whose one string literal holds `value`, as a bound or rewritten value would arrive.
    fn insert_of(value: &str, double: bool) -> Statement {
        struct Set(String, bool);
        impl VisitorMut for Set {
            type Break = ();
            fn pre_visit_value(&mut self, v: &mut ValueWithSpan) -> ControlFlow<()> {
                v.value = if self.1 {
                    Value::DoubleQuotedString(self.0.clone())
                } else {
                    Value::SingleQuotedString(self.0.clone())
                };
                ControlFlow::Continue(())
            }
        }
        let mut stmt = Parser::parse_sql(&MySqlDialect {}, "INSERT INTO t (a) VALUES ('p')")
            .expect("parse")
            .remove(0);
        let _ = stmt.visit(&mut Set(value.to_owned(), double));
        stmt
    }

    /// The one literal MySQL would read back from `text`.
    fn literal_of(text: &str) -> String {
        struct Get(Vec<String>);
        impl sqlparser::ast::Visitor for Get {
            type Break = ();
            fn pre_visit_value(&mut self, v: &ValueWithSpan) -> ControlFlow<()> {
                if let Value::SingleQuotedString(s) | Value::DoubleQuotedString(s) = &v.value {
                    self.0.push(s.clone());
                }
                ControlFlow::Continue(())
            }
        }
        use sqlparser::ast::Visit;
        let stmts =
            Parser::parse_sql(&MySqlDialect {}, text).unwrap_or_else(|e| panic!("{text}: {e}"));
        assert_eq!(stmts.len(), 1, "{text} is more than one statement");
        let mut get = Get(Vec::new());
        let _ = stmts[0].visit(&mut get);
        assert_eq!(get.0.len(), 1, "{text} holds {} literals", get.0.len());
        get.0.remove(0)
    }

    #[test]
    fn a_backslash_before_a_quote_cannot_end_the_literal() {
        let text = mysql_text(&insert_of("x\\' OR 1=1 -- ", false));
        assert_eq!(text, r"INSERT INTO t (a) VALUES ('x\\\' OR 1=1 -- ')");
    }

    #[test]
    fn every_literal_round_trips_through_mysql_text() {
        let values = [
            "plain",
            "it's",
            "a''b",
            "x\\' OR 1=1 -- ",
            "x\\",
            "\\\\'",
            "say \"hi\"",
            "\\\"",
            "'",
            "''",
            "\\",
            "nul\0byte",
            "ctrl\u{1a}z",
            "%s and %d",
        ];
        for value in values {
            for double in [false, true] {
                let text = mysql_text(&insert_of(value, double));
                assert_eq!(
                    literal_of(&text),
                    value,
                    "{value:?} (double: {double}) printed as {text}"
                );
            }
        }
    }
}
