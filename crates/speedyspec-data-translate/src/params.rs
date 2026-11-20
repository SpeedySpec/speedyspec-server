//! Placeholder numbering: `?` becomes `?1`, `?2`, … in the order MySQL binds them.

use std::ops::ControlFlow;

use speedyspec_data::Statement;
use sqlparser::ast::{Value, ValueWithSpan, VisitMut, VisitorMut};

struct Numbering(usize);

impl VisitorMut for Numbering {
    type Break = ();

    fn pre_visit_value(&mut self, value: &mut ValueWithSpan) -> ControlFlow<()> {
        if let Value::Placeholder(p) = &mut value.value
            && p == "?"
        {
            self.0 += 1;
            *p = format!("?{}", self.0);
        }
        ControlFlow::Continue(())
    }
}

/// Numbers every bare `?` in visiting order (MySQL's text order); a placeholder already numbered is left alone, so
/// numbering twice changes nothing.
pub fn number_placeholders(stmt: &mut Statement) {
    let already = {
        let mut highest = 0;
        let mut counter = Highest(&mut highest);
        let _ = sqlparser::ast::Visit::visit(stmt.tree(), &mut counter);
        highest
    };
    let _ = stmt.tree_mut().visit(&mut Numbering(already));
}

/// The highest number already used, so new numbers continue after it.
struct Highest<'a>(&'a mut usize);

impl sqlparser::ast::Visitor for Highest<'_> {
    type Break = ();

    fn pre_visit_value(&mut self, value: &ValueWithSpan) -> ControlFlow<()> {
        if let Value::Placeholder(p) = &value.value
            && let Some(n) = p.strip_prefix('?').and_then(|n| n.parse::<usize>().ok())
        {
            *self.0 = (*self.0).max(n);
        }
        ControlFlow::Continue(())
    }
}
