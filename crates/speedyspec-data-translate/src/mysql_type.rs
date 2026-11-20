//! MySQL column types: read from `sqlparser`'s data type, shown as MySQL 8.4's `SHOW COLUMNS` shows them.

use speedyspec_data::DbError;
use speedyspec_data_query::parse_one;
use sqlparser::ast::{DataType, Statement as Ast};

/// A MySQL column type: its base name, arguments (lengths, precision, enum values as quoted literals) and flags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MySqlType {
    pub base: String,
    pub args: Vec<String>,
    pub unsigned: bool,
    pub zerofill: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Temporal {
    Date,
    DateTime,
    Timestamp,
    Time,
    Year,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeFamily {
    Integer,
    Decimal { scale: u32 },
    Float,
    Text,
    Enum,
    Binary,
    Temporal(Temporal),
}

const INTEGERS: &[&str] = &["tinyint", "smallint", "mediumint", "int", "bigint"];

/// The comma-separated arguments of `(…)`, commas inside quotes kept.
fn split_args(inner: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    for c in inner.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => {
                quote = None;
                current.push(c);
            }
            (Some(_), c) => current.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                current.push(c);
            }
            (None, ',') => args.push(std::mem::take(&mut current).trim().to_string()),
            (None, c) => current.push(c),
        }
    }
    if !current.trim().is_empty() {
        args.push(current.trim().to_string());
    }
    args
}

impl MySqlType {
    /// From `sqlparser`'s data type, read through its MySQL text (one rule for its many MySQL variants).
    pub fn from_ast(dt: &DataType) -> Result<MySqlType, DbError> {
        MySqlType::from_text(&dt.to_string())
    }

    /// A type written as MySQL text, through `sqlparser`'s data-type parser.
    pub fn parse(text: &str) -> Result<MySqlType, DbError> {
        let (tree, _) =
            parse_one(&format!("CREATE TABLE speedyspec_type (c {text})"))?.into_parts();
        match tree {
            Ast::CreateTable(create) if create.columns.len() == 1 => {
                MySqlType::from_ast(&create.columns[0].data_type)
            }
            _ => Err(DbError::Parse {
                message: format!("not a column type: {text}"),
                line: 1,
                column: 1,
            }),
        }
    }

    fn from_text(text: &str) -> Result<MySqlType, DbError> {
        let text = text.trim();
        let (head, args, rest) = match text.find('(') {
            Some(open) => {
                let close =
                    text.rfind(')')
                        .filter(|c| *c > open)
                        .ok_or_else(|| DbError::Parse {
                            message: format!("unbalanced type: {text}"),
                            line: 1,
                            column: 1,
                        })?;
                (
                    &text[..open],
                    split_args(&text[open + 1..close]),
                    &text[close + 1..],
                )
            }
            None => (text, Vec::new(), ""),
        };
        let mut words: Vec<String> = head
            .split_whitespace()
            .map(str::to_ascii_lowercase)
            .collect();
        let mut flags: Vec<String> = rest
            .split_whitespace()
            .map(str::to_ascii_lowercase)
            .collect();
        // Flags written before any parenthesis (`INT UNSIGNED ZEROFILL`).
        while words
            .last()
            .is_some_and(|w| ["unsigned", "zerofill", "signed"].contains(&w.as_str()))
        {
            flags.push(words.pop().unwrap_or_default());
        }
        let zerofill = flags.iter().any(|f| f == "zerofill");
        let unsigned = zerofill || flags.iter().any(|f| f == "unsigned");
        let mut args = args;
        let base = match words.join(" ").as_str() {
            "integer" => "int".to_string(),
            "bool" | "boolean" => {
                args = vec!["1".into()];
                "tinyint".to_string()
            }
            "dec" | "numeric" | "fixed" => "decimal".to_string(),
            "double precision" | "real" => "double".to_string(),
            "character varying" => "varchar".to_string(),
            "character" => "char".to_string(),
            other => other.to_string(),
        };
        Ok(MySqlType {
            base,
            args,
            unsigned,
            zerofill,
        })
    }

    /// MySQL 8.4's `SHOW COLUMNS` form: lowercase, integer display widths dropped except `tinyint(1)` (and widths kept
    /// with `zerofill`), ` unsigned` and ` zerofill` suffixes.
    pub fn display(&self) -> String {
        let integer = INTEGERS.contains(&self.base.as_str());
        let keep_args = !self.args.is_empty()
            && (!integer || self.zerofill || (self.base == "tinyint" && self.args == ["1"]));
        let mut out = self.base.clone();
        if keep_args {
            out.push('(');
            out.push_str(&self.args.join(","));
            out.push(')');
        }
        if self.unsigned {
            out.push_str(" unsigned");
        }
        if self.zerofill {
            out.push_str(" zerofill");
        }
        out
    }

    pub fn family(&self) -> TypeFamily {
        match self.base.as_str() {
            b if INTEGERS.contains(&b) => TypeFamily::Integer,
            "decimal" => TypeFamily::Decimal {
                scale: self.args.get(1).and_then(|s| s.parse().ok()).unwrap_or(0),
            },
            "float" | "double" => TypeFamily::Float,
            "enum" | "set" => TypeFamily::Enum,
            "binary" | "varbinary" | "tinyblob" | "blob" | "mediumblob" | "longblob" | "bit" => {
                TypeFamily::Binary
            }
            "date" => TypeFamily::Temporal(Temporal::Date),
            "datetime" => TypeFamily::Temporal(Temporal::DateTime),
            "timestamp" => TypeFamily::Temporal(Temporal::Timestamp),
            "time" => TypeFamily::Temporal(Temporal::Time),
            "year" => TypeFamily::Temporal(Temporal::Year),
            _ => TypeFamily::Text,
        }
    }

    /// The value MySQL's non-strict mode stores for a `NOT NULL` column given none.
    pub fn implicit_default(&self) -> &str {
        match self.family() {
            TypeFamily::Integer | TypeFamily::Decimal { .. } | TypeFamily::Float => "0",
            TypeFamily::Text | TypeFamily::Binary => "",
            TypeFamily::Enum => match self.args.first() {
                Some(first) => first.trim_matches(|c| c == '\'' || c == '"'),
                None => "",
            },
            TypeFamily::Temporal(Temporal::Date) => "0000-00-00",
            TypeFamily::Temporal(Temporal::DateTime | Temporal::Timestamp) => "0000-00-00 00:00:00",
            TypeFamily::Temporal(Temporal::Time) => "00:00:00",
            TypeFamily::Temporal(Temporal::Year) => "0000",
        }
    }
}
