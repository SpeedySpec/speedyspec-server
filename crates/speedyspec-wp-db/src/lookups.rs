//! Charset lookups: `get_table_charset`, `get_col_charset`, `get_col_length`, `check_safe_collation`, and the
//! query-level charset check.

use std::cell::Cell;
use std::sync::OnceLock;

use regex::bytes::Regex;
use speedyspec_wp_hooks_contracts::{Count, HookName};

use crate::escape::string_value;
use crate::php::to_bytes;
use crate::{
    CharsetRules, ColLength, ColumnMeta, OutputShape, StripContext, WpDb, WpDbError, WpValue,
    get_table_from_query,
};

thread_local! {
    static CHECKING_COLLATION: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn skips_charset_check(query: &[u8]) -> bool {
    static SKIP: OnceLock<Regex> = OnceLock::new();
    let start = query
        .iter()
        .position(|b| !b"\r\n\t (".contains(b))
        .unwrap_or(query.len());
    SKIP.get_or_init(|| Regex::new(r"(?i-u)^(?:SHOW|DESCRIBE|DESC|EXPLAIN|CREATE)\s").unwrap())
        .is_match(&query[start..])
}

fn text(v: &Option<Vec<u8>>) -> String {
    v.as_deref()
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .unwrap_or_default()
}

/// A filter's answer read as `null` (`None`), `false` (`Some(None)`) or a charset.
fn charset_answer(v: &WpValue) -> Option<Option<String>> {
    match v {
        WpValue::Null => None,
        WpValue::Bool(false) => Some(None),
        other => Some(to_bytes(other).map(|b| String::from_utf8_lossy(&b).into_owned())),
    }
}

impl WpDb {
    pub(crate) fn connection_charset(&self) -> String {
        let charset = self.settings.borrow().charset.clone();
        if charset.is_empty() {
            self.config
                .charset
                .clone()
                .unwrap_or_else(|| "utf8mb4".into())
        } else {
            charset
        }
    }

    /// `wpdb::get_table_charset`; `None` where WordPress returns false (no charset: the table stores anything).
    pub fn get_table_charset(&self, table: &str) -> Result<Option<String>, WpDbError> {
        let key = table.to_lowercase();
        let hook = HookName::from("pre_get_table_charset");
        if !self.hooks.skip_if_empty(&hook, Count::Filters)? {
            let answer =
                self.hooks
                    .apply_filters(&hook, WpValue::Null, vec![WpValue::Str(table.into())])?;
            if let Some(answer) = charset_answer(&answer.read()) {
                return Ok(answer);
            }
        }
        if let Some(cached) = self.read(|s| s.table_charset.get(&key).cloned()) {
            return Ok(cached);
        }
        let quoted = format!("`{}`", table.split('.').collect::<Vec<_>>().join("`.`"));
        let results = self.get_results_sql(
            Some(format!("SHOW FULL COLUMNS FROM {quoted}").as_bytes()),
            Some(OutputShape::Object),
        )?;
        let rows = match results {
            Some(crate::Shaped::List(rows)) if !rows.is_empty() => rows,
            _ => {
                return Err(WpDbError::InvalidData(
                    self.host.translate("Could not retrieve table charset."),
                ));
            }
        };
        let mut columns: Vec<ColumnMeta> = Vec::new();
        for row in rows.iter() {
            let field = text(row.get("Field").unwrap_or(&None)).to_lowercase();
            let meta = ColumnMeta {
                field: field.clone(),
                sql_type: text(row.get("Type").unwrap_or(&None)),
                collation: row
                    .get("Collation")
                    .cloned()
                    .flatten()
                    .map(|b| String::from_utf8_lossy(&b).into_owned()),
            };
            match columns.iter_mut().find(|c| c.field == field) {
                Some(existing) => *existing = meta,
                None => columns.push(meta),
            }
        }
        let mut charsets: Vec<String> = Vec::new();
        let mut binary = false;
        for column in &columns {
            if let Some(collation) = column.collation.as_deref().filter(|c| !c.is_empty()) {
                let charset = collation.split('_').next().unwrap_or("").to_lowercase();
                if !charsets.contains(&charset) {
                    charsets.push(charset);
                }
            }
            let ty = column
                .sql_type
                .split('(')
                .next()
                .unwrap_or("")
                .to_uppercase();
            if [
                "BINARY",
                "VARBINARY",
                "TINYBLOB",
                "MEDIUMBLOB",
                "BLOB",
                "LONGBLOB",
            ]
            .contains(&ty.as_str())
            {
                binary = true;
                break;
            }
        }
        let k = key.clone();
        self.modify(|s| {
            s.col_meta.insert(k, columns);
        });
        let charset = if binary {
            Some("binary".to_string())
        } else {
            if let Some(i) = charsets.iter().position(|c| c == "utf8mb3") {
                charsets.remove(i);
                if !charsets.iter().any(|c| c == "utf8") {
                    charsets.push("utf8".into());
                }
            }
            match charsets.len() {
                0 => None,
                1 => Some(charsets[0].clone()),
                _ => {
                    charsets.retain(|c| c != "latin1");
                    let has = |c: &str| charsets.iter().any(|x| x == c);
                    if charsets.len() == 1 {
                        Some(charsets[0].clone())
                    } else if charsets.len() == 2 && has("utf8") && has("utf8mb4") {
                        Some("utf8".into())
                    } else {
                        Some("ascii".into())
                    }
                }
            }
        };
        let value = charset.clone();
        self.modify(|s| {
            s.table_charset.insert(key, value);
        });
        Ok(charset)
    }

    fn column_meta(&self, table: &str, column: &str) -> Option<Option<ColumnMeta>> {
        let (table, column) = (table.to_lowercase(), column.to_lowercase());
        self.read(|s| {
            s.col_meta
                .get(&table)
                .filter(|c| !c.is_empty())
                .map(|cols| cols.iter().find(|c| c.field == column).cloned())
        })
    }

    /// `wpdb::get_col_charset`; `None` where WordPress returns false (not a string column).
    pub fn get_col_charset(&self, table: &str, column: &str) -> Result<Option<String>, WpDbError> {
        let hook = HookName::from("pre_get_col_charset");
        if !self.hooks.skip_if_empty(&hook, Count::Filters)? {
            let answer = self.hooks.apply_filters(
                &hook,
                WpValue::Null,
                vec![WpValue::Str(table.into()), WpValue::Str(column.into())],
            )?;
            if let Some(answer) = charset_answer(&answer.read()) {
                return Ok(answer);
            }
        }
        // Skip this entirely if this isn't a MySQL database.
        if !self.settings.borrow().is_mysql {
            return Ok(None);
        }
        let key = table.to_lowercase();
        let primed = self.read(|s| {
            s.table_charset
                .get(&key)
                .is_some_and(|c| c.as_deref().is_some_and(|c| !c.is_empty()))
        });
        if !primed {
            self.get_table_charset(table)?;
        }
        let table_charset = self.read(|s| s.table_charset.get(&key).cloned().flatten());
        match self.column_meta(table, column) {
            None | Some(None) => Ok(table_charset),
            Some(Some(meta)) => Ok(meta
                .collation
                .filter(|c| !c.is_empty())
                .map(|c| c.split('_').next().unwrap_or("").to_string())),
        }
    }

    /// `wpdb::get_col_length`.
    pub fn get_col_length(
        &self,
        table: &str,
        column: &str,
    ) -> Result<Option<ColLength>, WpDbError> {
        if !self.settings.borrow().is_mysql {
            return Ok(None);
        }
        if self.read(|s| {
            s.col_meta
                .get(&table.to_lowercase())
                .is_none_or(|c| c.is_empty())
        }) {
            self.get_table_charset(table)?;
        }
        Ok(match self.column_meta(table, column) {
            Some(Some(meta)) => CharsetRules::col_length(&meta.sql_type),
            _ => None,
        })
    }

    pub(crate) fn strip_context<T>(&self, f: impl FnOnce(&StripContext) -> T) -> T {
        let charset = self.connection_charset();
        f(&StripContext {
            escape: &self.escape,
            connection_charset: &charset,
        })
    }

    /// The `SELECT CONVERT(..)` statement `strip_invalid_text` sends, run through `get_row(.., ARRAY_N)` as WordPress
    /// does (counted and filtered).
    pub(crate) fn convert_row(&self, sql: &[u8]) -> Result<Vec<Option<Vec<u8>>>, WpDbError> {
        self.modify(|s| s.check_current_query = false);
        self.query_bytes(sql)?;
        Ok(self
            .get_row(Some(OutputShape::ArrayN), 0)
            .map(|r| r.fields.into_iter().map(|(_, v)| v).collect())
            .unwrap_or_default())
    }

    /// `wpdb::strip_invalid_text_for_column`.
    pub fn strip_invalid_text_for_column(
        &self,
        table: &str,
        column: &str,
        value: &[u8],
    ) -> Result<Vec<u8>, WpDbError> {
        let Some(charset) = self
            .get_col_charset(table, column)?
            .filter(|c| !c.is_empty())
        else {
            return Ok(value.to_vec());
        };
        let length = self.get_col_length(table, column)?;
        let mut fields = vec![crate::Field {
            name: column.into(),
            value: string_value(value.to_vec()),
            format: "%s".into(),
            charset: Some(charset),
            length,
        }];
        self.strip_context(|cx| {
            CharsetRules::strip_invalid_text(&mut fields, cx, &mut |sql| self.convert_row(sql))
        })?;
        Ok(to_bytes(&fields[0].value).unwrap_or_default())
    }

    /// The query-level check: the query with what its table's charset cannot hold stripped.
    pub(crate) fn strip_invalid_text_from_query(&self, sql: &[u8]) -> Result<Vec<u8>, WpDbError> {
        if skips_charset_check(sql) {
            return Ok(sql.to_vec());
        }
        let charset = match get_table_from_query(sql) {
            Some(table) => match self.get_table_charset(&String::from_utf8_lossy(&table))? {
                Some(c) => c,
                // No charset: nothing to strip.
                None => return Ok(sql.to_vec()),
            },
            None => self.connection_charset(),
        };
        self.strip_context(|cx| {
            CharsetRules::strip_invalid_text_from_query(sql, &charset, cx, &mut |q| {
                self.convert_row(q)
            })
        })
    }

    /// `wpdb::check_safe_collation`.
    pub fn check_safe_collation(&self, query: &[u8]) -> bool {
        if CHECKING_COLLATION.get() {
            return true;
        }
        if skips_charset_check(query) || query.is_ascii() {
            return true;
        }
        let Some(table) = get_table_from_query(query) else {
            return false;
        };
        let table = String::from_utf8_lossy(&table).into_owned();
        CHECKING_COLLATION.set(true);
        let collation = self.get_table_charset(&table);
        CHECKING_COLLATION.set(false);
        match collation {
            Ok(None) => return true,
            Ok(Some(c)) if c == "latin1" => return true,
            _ => {}
        }
        let safe = [
            "utf8_bin",
            "utf8_general_ci",
            "utf8mb3_bin",
            "utf8mb3_general_ci",
            "utf8mb4_bin",
            "utf8mb4_general_ci",
        ];
        self.read(|s| {
            match s
                .col_meta
                .get(&table.to_lowercase())
                .filter(|c| !c.is_empty())
            {
                None => false,
                Some(cols) => cols.iter().all(|c| {
                    c.collation
                        .as_deref()
                        .is_none_or(|k| k.is_empty() || safe.contains(&k))
                }),
            }
        })
    }
}
