//! A statement's shape: its text with every literal a placeholder, so the corpus holds each statement form once.

use std::collections::{BTreeMap, BTreeSet};
use std::io::BufRead;
use std::ops::ControlFlow;

use serde::{Deserialize, Serialize};
use speedyspec_data::DbError;
use speedyspec_data_query::parse_one;
use sqlparser::ast::{Expr, InTarget, SetExpr, Value, VisitMut, VisitorMut};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorpusEntry {
    /// `None` when the statement does not parse.
    pub shape: Option<String>,
    pub example: String,
    pub sources: BTreeSet<String>,
    pub count: u64,
}

fn placeholder() -> Expr {
    Expr::value(Value::Placeholder("?".into()))
}

fn is_placeholder(expr: &Expr) -> bool {
    matches!(expr, Expr::Value(v) if matches!(&v.value, Value::Placeholder(p) if p == "?"))
}

struct Shape;

impl VisitorMut for Shape {
    type Break = ();

    fn post_visit_expr(&mut self, expr: &mut Expr) -> ControlFlow<()> {
        match expr {
            Expr::Value(v) if !matches!(v.value, Value::Null) => *expr = placeholder(),
            Expr::In {
                target: InTarget::List(list),
                ..
            } if list.iter().all(is_placeholder) => {
                *list = vec![placeholder()];
            }
            _ => {}
        }
        ControlFlow::Continue(())
    }

    fn post_visit_query(&mut self, query: &mut sqlparser::ast::Query) -> ControlFlow<()> {
        // A multi-row VALUES of placeholders is one shape however many rows it has.
        if let SetExpr::Values(values) = query.body.as_mut()
            && values.rows.len() > 1
            && values.rows.windows(2).all(|w| w[0].content == w[1].content)
        {
            values.rows.truncate(1);
        }
        ControlFlow::Continue(())
    }
}

/// `sql` with every literal and placeholder as `?`, an `IN` list of only those as `IN (?)`, printed again.
pub fn shape_of(sql: &str) -> Result<String, DbError> {
    let mut stmt = parse_one(sql)?;
    let _ = stmt.tree_mut().visit(&mut Shape);
    Ok(stmt.tree().to_string())
}

#[derive(Deserialize)]
struct Line {
    sql: String,
    source: String,
}

/// Adds JSON lines `{"sql", "source"}` to `into`, keyed by shape (or by the text when it does not parse); the first
/// text of a shape is its example. Lines that are not such JSON are skipped.
pub fn ingest(lines: impl BufRead, into: &mut BTreeMap<String, CorpusEntry>) {
    for line in lines.lines().map_while(Result::ok) {
        let Ok(Line { sql, source }) = serde_json::from_str::<Line>(&line) else {
            continue;
        };
        let sql = sql.trim().to_string();
        if sql.is_empty() {
            continue;
        }
        let shape = shape_of(&sql).ok();
        let key = shape.clone().unwrap_or_else(|| sql.clone());
        let entry = into.entry(key).or_insert_with(|| CorpusEntry {
            shape,
            example: sql,
            sources: BTreeSet::new(),
            count: 0,
        });
        entry.sources.insert(source);
        entry.count += 1;
    }
}
