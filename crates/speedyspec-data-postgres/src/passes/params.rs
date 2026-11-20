//! Numbered placeholders (`?N`) moved between statements: copied to new numbers, or renumbered for a statement that
//! carries only the values it uses.

use std::ops::ControlFlow;

use speedyspec_data::DbValue;
use sqlparser::ast::{Value, ValueWithSpan, VisitorMut};

fn number(p: &str) -> Option<usize> {
    p.strip_prefix('?').and_then(|n| n.parse().ok())
}

struct Map<F: FnMut(usize) -> usize>(F);

impl<F: FnMut(usize) -> usize> VisitorMut for Map<F> {
    type Break = ();

    fn pre_visit_value(&mut self, value: &mut ValueWithSpan) -> ControlFlow<()> {
        if let Value::Placeholder(p) = &mut value.value
            && let Some(n) = number(p)
        {
            *p = format!("?{}", (self.0)(n));
        }
        ControlFlow::Continue(())
    }
}

/// Gives every placeholder in `node` a new number past `params`, appending a copy of its value: the copy binds
/// apart from the original, as PostgreSQL types each parameter once.
pub(crate) fn copy_params<V: sqlparser::ast::VisitMut>(node: &mut V, params: &mut Vec<DbValue>) {
    let mut added: Vec<(usize, usize)> = Vec::new();
    let original = params.clone();
    let _ = node.visit(&mut Map(|n| {
        if let Some((_, new)) = added.iter().find(|(old, _)| *old == n) {
            return *new;
        }
        let value = original
            .get(n.wrapping_sub(1))
            .cloned()
            .unwrap_or(DbValue::Null);
        params.push(value);
        let new = params.len();
        added.push((n, new));
        new
    }));
}

/// Numbers `node`'s placeholders 1, 2, … in order of first use, and returns the values they now stand for.
pub(crate) fn renumber<V: sqlparser::ast::VisitMut>(
    node: &mut V,
    params: &[DbValue],
) -> Vec<DbValue> {
    let mut seen: Vec<usize> = Vec::new();
    let _ = node.visit(&mut Map(|n| match seen.iter().position(|&o| o == n) {
        Some(i) => i + 1,
        None => {
            seen.push(n);
            seen.len()
        }
    }));
    seen.into_iter()
        .map(|n| {
            params
                .get(n.wrapping_sub(1))
                .cloned()
                .unwrap_or(DbValue::Null)
        })
        .collect()
}
