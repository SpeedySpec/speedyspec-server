//! Result rows as WordPress hands them out, and the shapes of `get_results`.

use std::ops::ControlFlow;
use std::rc::Rc;

use speedyspec_data::ast::{Value, VisitMut, VisitorMut};
use speedyspec_data::{DbValue, Statement};
use speedyspec_data_query::mysql_text;
use speedyspec_wp_db_contracts::WpRow;

use crate::escape::real_escape;
use crate::php::float_to_string;

/// `get_results`' and `get_row`'s output types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputShape {
    Object,
    ObjectK,
    ArrayA,
    ArrayN,
}

impl OutputShape {
    /// `OBJECT_K`, `ARRAY_A` and `ARRAY_N` exactly; `OBJECT` in any case, as WordPress's back-compat check does.
    pub fn parse(output: &str) -> Option<OutputShape> {
        match output {
            "OBJECT_K" => Some(OutputShape::ObjectK),
            "ARRAY_A" => Some(OutputShape::ArrayA),
            "ARRAY_N" => Some(OutputShape::ArrayN),
            o if o.eq_ignore_ascii_case("OBJECT") => Some(OutputShape::Object),
            _ => None,
        }
    }
}

/// Rows in a shape: a list, or keyed by the first column (`OBJECT_K`, first row per key kept).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shaped {
    List(Vec<WpRow>),
    Keyed(Vec<(Vec<u8>, WpRow)>),
}

impl Shaped {
    pub fn of(rows: &[WpRow], shape: OutputShape) -> Shaped {
        match shape {
            OutputShape::ObjectK => Shaped::Keyed(
                keyed_rows(rows)
                    .into_iter()
                    .map(|(key, index)| (key, rows[index].clone()))
                    .collect(),
            ),
            _ => Shaped::List(rows.to_vec()),
        }
    }
}

/// `OBJECT_K`'s keys and the index of the first row with each, in order (one pass, keys hashed).
pub fn keyed_rows(rows: &[WpRow]) -> Vec<(Vec<u8>, usize)> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        let key = row
            .fields
            .first()
            .and_then(|(_, v)| v.clone())
            .unwrap_or_default();
        if seen.insert(key.clone()) {
            out.push((key, index));
        }
    }
    out
}

/// `get_var`'s cell: null for a missing row or column, SQL NULL and `''`.
pub fn var(rows: &[WpRow], x: usize, y: usize) -> Option<Vec<u8>> {
    rows.get(y)?
        .fields
        .get(x)?
        .1
        .clone()
        .filter(|v| !v.is_empty())
}

/// `$wpdb->last_query`: the text that ran, or a statement rendered only when read.
#[derive(Debug, Clone, PartialEq)]
pub enum LastQuery {
    Text(Vec<u8>),
    Statement(Rc<Statement>),
}

const MARK: char = '\u{1}';

struct Mark(usize);

impl VisitorMut for Mark {
    type Break = ();
    fn pre_visit_value(
        &mut self,
        value: &mut speedyspec_data::ast::ValueWithSpan,
    ) -> ControlFlow<()> {
        if matches!(&value.value, Value::Placeholder(p) if p == "?") {
            value.value = Value::Placeholder(format!("{MARK}{}{MARK}", self.0));
            self.0 += 1;
        }
        ControlFlow::Continue(())
    }
}

fn literal(value: &DbValue) -> Vec<u8> {
    match value {
        DbValue::Null => b"NULL".to_vec(),
        DbValue::Int(i) => i.to_string().into_bytes(),
        DbValue::UInt(u) => u.to_string().into_bytes(),
        DbValue::Float(f) => float_to_string(*f).into_bytes(),
        DbValue::Text(t) => quoted(t.as_bytes()),
        DbValue::Bytes(b) => quoted(b),
    }
}

fn quoted(bytes: &[u8]) -> Vec<u8> {
    let mut out = vec![b'\''];
    out.extend(real_escape(bytes));
    out.push(b'\'');
    out
}

impl LastQuery {
    /// The text: a statement as MySQL text with its params inlined by `real_escape`.
    pub fn text(&self) -> Vec<u8> {
        match self {
            LastQuery::Text(t) => t.clone(),
            LastQuery::Statement(stmt) => {
                let mut tree = stmt.tree().clone();
                let _ = tree.visit(&mut Mark(0));
                let text = mysql_text(&tree);
                let mut out = Vec::with_capacity(text.len());
                let mut parts = text.split(MARK);
                if let Some(first) = parts.next() {
                    out.extend_from_slice(first.as_bytes());
                }
                while let (Some(index), Some(rest)) = (parts.next(), parts.next()) {
                    match index
                        .parse::<usize>()
                        .ok()
                        .and_then(|i| stmt.params().get(i))
                    {
                        Some(param) => out.extend(literal(param)),
                        None => out.push(b'?'),
                    }
                    out.extend_from_slice(rest.as_bytes());
                }
                out
            }
        }
    }
}
