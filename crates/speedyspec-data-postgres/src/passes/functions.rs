//! MySQL's functions as PostgreSQL expressions, built from templates whose `__aN` names stand for the arguments.

use std::ops::ControlFlow;

use speedyspec_data::{DbError, DialectKind};
use sqlparser::ast::{
    Expr, Function, FunctionArg, FunctionArgExpr, FunctionArguments, Value, VisitMut, VisitorMut,
};
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::parser::Parser;

pub(crate) fn unsupported(construct: &str) -> DbError {
    DbError::Unsupported {
        construct: construct.into(),
        dialect: DialectKind::Postgres,
    }
}

struct Fill(Vec<Expr>);

impl VisitorMut for Fill {
    type Break = ();

    fn post_visit_expr(&mut self, expr: &mut Expr) -> ControlFlow<()> {
        if let Expr::Identifier(i) = expr
            && let Some(n) = i
                .value
                .strip_prefix("__a")
                .and_then(|n| n.parse::<usize>().ok())
            && let Some(arg) = self.0.get(n)
        {
            *expr = arg.clone();
        }
        ControlFlow::Continue(())
    }
}

/// The PostgreSQL expression `sql`, with `__a0`, `__a1`… replaced by `args`.
pub(crate) fn template(sql: &str, args: Vec<Expr>) -> Expr {
    let mut expr = Parser::new(&PostgreSqlDialect {})
        .try_with_sql(sql)
        .and_then(|mut p| p.parse_expr())
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
    let _ = expr.visit(&mut Fill(args));
    expr
}

/// The plain arguments of a call.
pub(crate) fn args(f: &Function) -> Vec<Expr> {
    match &f.args {
        FunctionArguments::List(list) => list
            .args
            .iter()
            .filter_map(|a| match a {
                FunctionArg::Unnamed(FunctionArgExpr::Expr(e)) => Some(e.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn number(expr: &Expr) -> Option<i64> {
    match expr {
        Expr::Value(v) => match &v.value {
            Value::Number(n, _) => n.parse().ok(),
            _ => None,
        },
        Expr::UnaryOp {
            op: sqlparser::ast::UnaryOperator::Minus,
            expr,
        } => number(expr).map(|n| -n),
        Expr::Nested(inner) => number(inner),
        _ => None,
    }
}

fn text(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Value(v) => match &v.value {
            Value::SingleQuotedString(s) | Value::DoubleQuotedString(s) => Some(s),
            _ => None,
        },
        _ => None,
    }
}

fn is_string_literal(expr: &Expr) -> bool {
    text(expr).is_some()
}

/// `DATE_FORMAT`'s specifiers as `to_char`'s; letters that are not a specifier are quoted.
fn date_format(format: &str) -> Result<String, DbError> {
    let mut out = String::new();
    let mut letters = String::new();
    let flush = |letters: &mut String, out: &mut String| {
        if !letters.is_empty() {
            out.push('"');
            out.push_str(letters);
            out.push('"');
            letters.clear();
        }
    };
    let mut chars = format.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            flush(&mut letters, &mut out);
            let spec = chars
                .next()
                .ok_or_else(|| unsupported("DATE_FORMAT with a trailing %"))?;
            out.push_str(match spec {
                'Y' => "YYYY",
                'y' => "YY",
                'm' => "MM",
                'c' => "FMMM",
                'd' => "DD",
                'e' => "FMDD",
                'H' => "HH24",
                'h' => "HH12",
                'i' => "MI",
                's' => "SS",
                'p' => "AM",
                'j' => "DDD",
                'M' => "FMMonth",
                'b' => "Mon",
                'W' => "FMDay",
                'a' => "Dy",
                '%' => "%",
                other => return Err(unsupported(&format!("DATE_FORMAT specifier %{other}"))),
            });
        } else if c.is_alphabetic() {
            letters.push(c);
        } else {
            flush(&mut letters, &mut out);
            out.push(c);
        }
    }
    flush(&mut letters, &mut out);
    Ok(out)
}

fn quoted(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// The PostgreSQL form of the MySQL call `f` (`truthy` turns a MySQL condition into a boolean); `None` when the call
/// needs no change.
pub(crate) fn rewrite_call(
    f: &Function,
    truthy: &dyn Fn(Expr) -> Expr,
) -> Result<Option<Expr>, DbError> {
    let name = speedyspec_data_translate::object_name(&f.name).to_ascii_lowercase();
    let a = args(f);
    let arg = |i: usize| {
        a.get(i)
            .cloned()
            .ok_or_else(|| unsupported(&format!("{name} with too few arguments")))
    };
    let t = |sql: &str, args: Vec<Expr>| Ok(Some(template(sql, args)));
    match name.as_str() {
        "now" | "current_timestamp" | "localtimestamp" | "sysdate" => {
            t("LOCALTIMESTAMP(0)", vec![])
        }
        "curdate" | "current_date" => t("CURRENT_DATE", vec![]),
        "curtime" | "current_time" => t("LOCALTIME(0)", vec![]),
        "utc_timestamp" => t("(now() AT TIME ZONE 'UTC')::timestamp(0)", vec![]),
        "unix_timestamp" if a.is_empty() => t("floor(extract(epoch FROM now()))::bigint", vec![]),
        "unix_timestamp" => t("floor(extract(epoch FROM __a0))::bigint", vec![arg(0)?]),
        "from_unixtime" if a.len() == 1 => t(
            "(to_timestamp(__a0) AT TIME ZONE 'UTC')::timestamp(0)",
            vec![arg(0)?],
        ),
        "speedyspec_date_add" | "speedyspec_date_sub" => {
            let unit = text(&arg(2)?)
                .map(str::to_ascii_lowercase)
                .ok_or_else(|| unsupported("an interval unit"))?;
            let unit = match unit.as_str() {
                "quarter" => "3 months".to_string(),
                "microsecond" | "second" | "minute" | "hour" | "day" | "week" | "month"
                | "year" => format!("1 {unit}"),
                other => return Err(unsupported(&format!("INTERVAL unit {other}"))),
            };
            let op = if name.ends_with("add") { "+" } else { "-" };
            t(
                &format!("(__a0 {op} __a1 * INTERVAL {})", quoted(&unit)),
                vec![arg(0)?, arg(1)?],
            )
        }
        "year" | "month" | "day" | "dayofmonth" | "hour" | "minute" | "second" if a.len() == 1 => {
            let field = if name == "dayofmonth" {
                "day"
            } else {
                name.as_str()
            };
            t(
                &format!("extract({} FROM __a0)::integer", field.to_ascii_uppercase()),
                vec![arg(0)?],
            )
        }
        "dayofweek" => t("(extract(DOW FROM __a0)::integer + 1)", vec![arg(0)?]),
        "week" => {
            let mode = match a.get(1) {
                None => 0,
                Some(m) => number(m).ok_or_else(|| unsupported("WEEK with a non-literal mode"))?,
            };
            match mode {
                0 => t(
                    "floor((extract(DOY FROM __a0) + 6 - extract(DOW FROM __a0)) / 7)::integer",
                    vec![arg(0)?],
                ),
                1 => t(
                    "CASE WHEN extract(ISOYEAR FROM __a0) < extract(YEAR FROM __a0) THEN 0 WHEN extract(ISOYEAR FROM __a0) > extract(YEAR FROM __a0) THEN 53 ELSE extract(WEEK FROM __a0)::integer END",
                    vec![arg(0)?],
                ),
                3 => t("extract(WEEK FROM __a0)::integer", vec![arg(0)?]),
                other => Err(unsupported(&format!("WEEK mode {other}"))),
            }
        }
        "date_format" => {
            let format = text(&arg(1)?)
                .ok_or_else(|| unsupported("DATE_FORMAT with a non-literal format"))?
                .to_string();
            t(
                &format!("to_char(__a0, {})", quoted(&date_format(&format)?)),
                vec![arg(0)?],
            )
        }
        "concat" => {
            let parts: Vec<String> = (0..a.len())
                .map(|i| {
                    if is_string_literal(&a[i]) {
                        format!("__a{i}")
                    } else {
                        format!("__a{i}::text")
                    }
                })
                .collect();
            t(&format!("({})", parts.join(" || ")), a.clone())
        }
        "group_concat" => {
            let FunctionArguments::List(list) = &f.args else {
                return Ok(None);
            };
            let mut separator = Expr::value(Value::SingleQuotedString(",".into()));
            let mut order = None;
            for clause in &list.clauses {
                match clause {
                    sqlparser::ast::FunctionArgumentClause::Separator(v) => {
                        separator = Expr::value(v.value.clone())
                    }
                    sqlparser::ast::FunctionArgumentClause::OrderBy(o) => order = Some(o.clone()),
                    _ => {}
                }
            }
            let distinct = matches!(
                list.duplicate_treatment,
                Some(sqlparser::ast::DuplicateTreatment::Distinct)
            );
            let value = if a.len() > 1 {
                let parts: Vec<String> = (0..a.len()).map(|i| format!("__a{i}::text")).collect();
                template(&format!("({})", parts.join(" || ")), a.clone())
            } else {
                arg(0)?
            };
            let mut call = template(
                &format!(
                    "string_agg({}__a0::text, __a1)",
                    if distinct { "DISTINCT " } else { "" }
                ),
                vec![value, separator],
            );
            if let (Some(order), Expr::Function(g)) = (order, &mut call)
                && let FunctionArguments::List(list) = &mut g.args
            {
                list.clauses
                    .push(sqlparser::ast::FunctionArgumentClause::OrderBy(order));
            }
            Ok(Some(call))
        }
        "if" => t(
            "CASE WHEN __a0 THEN __a1 ELSE __a2 END",
            vec![truthy(arg(0)?), arg(1)?, arg(2)?],
        ),
        "ifnull" => t("COALESCE(__a0, __a1)", vec![arg(0)?, arg(1)?]),
        "field" => {
            let whens: Vec<String> = (1..a.len())
                .map(|i| format!("WHEN __a{i} THEN {i}"))
                .collect();
            t(
                &format!("CASE __a0 {} ELSE 0 END", whens.join(" ")),
                a.clone(),
            )
        }
        "greatest" | "least" => {
            let nulls: Vec<String> = (0..a.len()).map(|i| format!("__a{i} IS NULL")).collect();
            let list: Vec<String> = (0..a.len()).map(|i| format!("__a{i}")).collect();
            t(
                &format!(
                    "CASE WHEN {} THEN NULL ELSE {}({}) END",
                    nulls.join(" OR "),
                    name.to_ascii_uppercase(),
                    list.join(", ")
                ),
                a.clone(),
            )
        }
        "rand" if a.is_empty() => t("random()", vec![]),
        "rand" => Err(unsupported("RAND(seed)")),
        "length" => t("octet_length(__a0)", vec![arg(0)?]),
        "lcase" => t("lower(__a0)", vec![arg(0)?]),
        "ucase" => t("upper(__a0)", vec![arg(0)?]),
        "locate" if a.len() == 2 => t("position(__a0 IN __a1)", vec![arg(0)?, arg(1)?]),
        "locate" => Err(unsupported("LOCATE with a start position")),
        "instr" => t("strpos(__a0, __a1)", vec![arg(0)?, arg(1)?]),
        "substring_index" => {
            let k = number(&arg(2)?)
                .ok_or_else(|| unsupported("SUBSTRING_INDEX with a non-literal count"))?;
            match k {
                1 => t("split_part(__a0, __a1, 1)", vec![arg(0)?, arg(1)?]),
                -1 => t("split_part(__a0, __a1, -1)", vec![arg(0)?, arg(1)?]),
                k if k > 1 => t(
                    &format!("array_to_string((string_to_array(__a0, __a1))[1:{k}], __a1)"),
                    vec![arg(0)?, arg(1)?],
                ),
                k if k < -1 => t(
                    &format!(
                        "array_to_string((string_to_array(__a0, __a1))[greatest(cardinality(string_to_array(__a0, __a1)) - {} + 1, 1):], __a1)",
                        -k
                    ),
                    vec![arg(0)?, arg(1)?],
                ),
                _ => t("''", vec![]),
            }
        }
        "last_insert_id" if a.is_empty() => t("lastval()", vec![]),
        "database" | "schema" => t("current_database()", vec![]),
        "uuid" => t("gen_random_uuid()::text", vec![]),
        "isnull" => t("__a0 IS NULL", vec![arg(0)?]),
        _ => Ok(None),
    }
}
