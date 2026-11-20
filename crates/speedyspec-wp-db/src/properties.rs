//! `wpdb`'s properties, read and written by name (what PHP's property hooks and magic methods forward to).

use std::rc::Rc;

use indexmap::IndexMap;
use speedyspec_hooks::Key;

use crate::escape::string_value;
use crate::php::{to_bytes, to_int};
use crate::{LastQuery, WpDb, WpValue};

fn list(items: impl IntoIterator<Item = WpValue>) -> WpValue {
    WpValue::Array(
        items
            .into_iter()
            .enumerate()
            .map(|(i, v)| (Key::Int(i as i64), v))
            .collect(),
    )
}

fn strings(items: &[String]) -> WpValue {
    list(items.iter().map(|s| WpValue::Str(s.clone())))
}

fn text(v: &WpValue) -> String {
    to_bytes(v)
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default()
}

fn truthy(v: &WpValue) -> bool {
    match v {
        WpValue::Null | WpValue::Bool(false) | WpValue::Int(0) => false,
        WpValue::Array(a) => !a.is_empty(),
        other => to_bytes(other).is_some_and(|b| !b.is_empty() && b != b"0"),
    }
}

fn string_list(v: &WpValue) -> Vec<String> {
    match v {
        WpValue::Array(items) => items.iter().map(|(_, v)| text(v)).collect(),
        _ => Vec::new(),
    }
}

/// The table-name properties WordPress declares.
pub const TABLE_PROPERTIES: [&str; 22] = [
    "comments",
    "commentmeta",
    "links",
    "options",
    "postmeta",
    "posts",
    "terms",
    "term_relationships",
    "term_taxonomy",
    "termmeta",
    "usermeta",
    "users",
    "blogs",
    "blogmeta",
    "registration_log",
    "signups",
    "site",
    "sitecategories",
    "sitemeta",
    "categories",
    "post2cat",
    "link2cat",
];

/// The properties PHP sees as protected or private (reached through `__get` and friends).
pub const HIDDEN_PROPERTIES: [&str; 16] = [
    "result",
    "col_meta",
    "table_charset",
    "check_current_query",
    "checking_collation",
    "col_info",
    "reconnect_retries",
    "dbuser",
    "dbpassword",
    "dbname",
    "dbhost",
    "dbh",
    "incompatible_modes",
    "allow_unsafe_unquoted_parameters",
    "use_mysqli",
    "has_connected",
];

/// Writes WordPress's `__set` ignores.
const READ_ONLY: [&str; 4] = [
    "col_meta",
    "table_charset",
    "check_current_query",
    "allow_unsafe_unquoted_parameters",
];

impl WpDb {
    /// The property `name`, or `None` when `wpdb` has no such property. `last_result` is its rows as arrays,
    /// `col_info` its columns as arrays, `error` `[code, message]`; the facade makes the objects.
    pub fn property(&self, name: &str) -> Option<WpValue> {
        let s = |f: &dyn Fn(&crate::RequestState) -> WpValue| self.read(|st| f(st));
        Some(match name {
            "show_errors" => s(&|st| WpValue::Bool(st.show_errors)),
            "suppress_errors" => s(&|st| WpValue::Bool(st.suppress_errors)),
            "last_error" => s(&|st| WpValue::Str(st.last_error.clone())),
            "num_queries" => s(&|st| WpValue::Int(st.num_queries as i64)),
            "num_rows" => s(&|st| WpValue::Int(st.num_rows as i64)),
            "rows_affected" => s(&|st| WpValue::Int(st.rows_affected as i64)),
            "insert_id" => s(&|st| WpValue::Int(st.insert_id as i64)),
            "last_query" => s(&|st| {
                st.last_query
                    .as_ref()
                    .map_or(WpValue::Null, |q| string_value(q.text()))
            }),
            "func_call" => s(&|st| match (&st.func_call, &st.last_query) {
                (Some(call), _) => WpValue::Str(call.clone()),
                (None, Some(q @ LastQuery::Statement(_))) => WpValue::Str(format!(
                    "$db->query(\"{}\")",
                    String::from_utf8_lossy(&q.text())
                )),
                _ => WpValue::Null,
            }),
            "time_start" => s(&|st| st.time_start.map_or(WpValue::Null, WpValue::Float)),
            "ready" => s(&|st| WpValue::Bool(st.ready)),
            "queries" => s(&|st| {
                if st.queries.is_empty() {
                    return WpValue::Null;
                }
                list(st.queries.iter().map(|q| {
                    list([
                        WpValue::Str(q.query.clone()),
                        WpValue::Float(q.time),
                        WpValue::Str(q.caller.clone()),
                        WpValue::Float(q.start),
                        q.data.clone(),
                    ])
                }))
            }),
            "error" => s(&|st| match &st.error {
                Some((code, message)) => {
                    list([WpValue::Str(code.clone()), WpValue::Str(message.clone())])
                }
                None => WpValue::Null,
            }),
            "last_result" => s(&|st| list(st.last_result.iter().map(row_assoc))),
            "col_info" => s(&|st| match &st.col_info {
                Some(cols) if !cols.is_empty() => list(cols.iter().map(|c| {
                    WpValue::Array(vec![
                        (Key::Str("name".into()), WpValue::Str(c.name.clone())),
                        (Key::Str("orgname".into()), WpValue::Str(c.org_name.clone())),
                        (Key::Str("table".into()), WpValue::Str(c.table.clone())),
                        (
                            Key::Str("orgtable".into()),
                            WpValue::Str(c.org_table.clone()),
                        ),
                        (Key::Str("def".into()), WpValue::Str(String::new())),
                        (
                            Key::Str("db".into()),
                            WpValue::Str(self.config.name.clone()),
                        ),
                        (Key::Str("catalog".into()), WpValue::Str("def".into())),
                        (
                            Key::Str("max_length".into()),
                            WpValue::Int(c.max_length as i64),
                        ),
                        (Key::Str("length".into()), WpValue::Int(c.length as i64)),
                        (
                            Key::Str("charsetnr".into()),
                            WpValue::Int(i64::from(c.charset)),
                        ),
                        (Key::Str("flags".into()), WpValue::Int(i64::from(c.flags))),
                        (Key::Str("type".into()), self.get_col_info_one(c, "type")),
                        (
                            Key::Str("decimals".into()),
                            WpValue::Int(i64::from(c.decimals)),
                        ),
                    ])
                })),
                _ => WpValue::Null,
            }),
            "col_meta" => s(&|st| {
                WpValue::Array(
                    st.col_meta
                        .iter()
                        .map(|(table, cols)| {
                            let cols = cols
                                .iter()
                                .map(|c| {
                                    let entry = WpValue::Array(vec![
                                        (Key::Str("Field".into()), WpValue::Str(c.field.clone())),
                                        (Key::Str("Type".into()), WpValue::Str(c.sql_type.clone())),
                                        (
                                            Key::Str("Collation".into()),
                                            c.collation.clone().map_or(WpValue::Null, WpValue::Str),
                                        ),
                                    ]);
                                    (Key::Str(c.field.clone()), entry)
                                })
                                .collect();
                            (Key::Str(table.clone()), WpValue::Array(cols))
                        })
                        .collect(),
                )
            }),
            "table_charset" => s(&|st| {
                WpValue::Array(
                    st.table_charset
                        .iter()
                        .map(|(t, c)| {
                            (
                                Key::Str(t.clone()),
                                c.clone().map_or(WpValue::Bool(false), WpValue::Str),
                            )
                        })
                        .collect(),
                )
            }),
            "check_current_query" => s(&|st| WpValue::Bool(st.check_current_query)),
            "dbh" => s(&|st| WpValue::Bool(st.dbh_present)),
            "result" => s(&|st| WpValue::Bool(st.col_info.is_some())),
            "checking_collation" => WpValue::Bool(false),
            "reconnect_retries" => WpValue::Int(i64::from(self.reconnect_retries())),
            "dbuser" => WpValue::Str(self.config.user.clone()),
            "dbpassword" => WpValue::Str(self.config.password.clone()),
            "dbname" => WpValue::Str(self.config.name.clone()),
            "dbhost" => WpValue::Str(self.config.host.clone()),
            "incompatible_modes" => strings(&self.settings.borrow().incompatible_modes),
            "allow_unsafe_unquoted_parameters" => {
                WpValue::Bool(self.settings.borrow().allow_unsafe_unquoted_parameters)
            }
            "use_mysqli" => WpValue::Bool(true),
            "is_mysql" => {
                if self.settings.borrow().is_mysql {
                    WpValue::Bool(true)
                } else {
                    WpValue::Null
                }
            }
            "has_connected" => WpValue::Bool(self.settings.borrow().has_connected),
            "field_types" => WpValue::Array(
                self.field_types()
                    .into_iter()
                    .map(|(k, v)| (Key::Str(k), WpValue::Str(v)))
                    .collect(),
            ),
            "charset" => WpValue::Str(self.charset()),
            "collate" => WpValue::Str(self.collate()),
            "prefix" => WpValue::Str(self.with_tables(|t| t.prefix().to_string())),
            "base_prefix" => self.with_tables(|t| {
                t.base_prefix()
                    .map_or(WpValue::Null, |p| WpValue::Str(p.into()))
            }),
            "blogid" => WpValue::Int(self.with_tables(|t| t.blogid())),
            "siteid" => WpValue::Int(self.with_tables(|t| t.siteid())),
            "tables" => self.with_tables(|t| strings(t.tables_list())),
            "old_tables" => self.with_tables(|t| strings(t.old_tables())),
            "global_tables" => self.with_tables(|t| strings(t.global_tables())),
            "ms_global_tables" => self.with_tables(|t| strings(t.ms_global_tables())),
            "old_ms_global_tables" => self.with_tables(|t| strings(t.old_ms_global_tables())),
            other if TABLE_PROPERTIES.contains(&other) => self.with_tables(|t| {
                t.get(other)
                    .map_or(WpValue::Null, |n| WpValue::Str(n.into()))
            }),
            _ => return None,
        })
    }

    fn get_col_info_one(&self, c: &speedyspec_data::ColumnInfo, info: &str) -> WpValue {
        let columns = Rc::new(vec![c.clone()]);
        let saved = self.read(|s| s.col_info.clone());
        self.modify(|s| s.col_info = Some(columns));
        let value = self.get_col_info(info, 0);
        self.modify(|s| s.col_info = saved);
        value
    }

    /// Writes the property `name`; false when `wpdb` has no such property (the caller keeps it as a dynamic one).
    /// Writes WordPress's `__set` ignores are accepted and dropped.
    pub fn set_property(&self, name: &str, value: &WpValue) -> bool {
        if READ_ONLY.contains(&name) {
            return true;
        }
        match name {
            "show_errors" => self.modify(|s| s.show_errors = truthy(value)),
            "suppress_errors" => self.modify(|s| s.suppress_errors = truthy(value)),
            "last_error" => {
                let v = text(value);
                self.modify(|s| s.last_error = v)
            }
            "num_queries" => self.modify(|s| s.num_queries = to_int(value).max(0) as u64),
            "num_rows" => self.modify(|s| s.num_rows = to_int(value).max(0) as u64),
            "rows_affected" => self.modify(|s| s.rows_affected = to_int(value).max(0) as u64),
            "insert_id" => self.modify(|s| s.insert_id = to_int(value).max(0) as u64),
            "last_query" => {
                let q = to_bytes(value).filter(|_| !matches!(value, WpValue::Null));
                self.modify(|s| s.last_query = q.clone().map(LastQuery::Text))
            }
            "func_call" => {
                let v = text(value);
                self.modify(|s| s.func_call = Some(v))
            }
            "time_start" => {
                let v = crate::php::to_float(value);
                self.modify(|s| s.time_start = Some(v))
            }
            "ready" => self.modify(|s| s.ready = truthy(value)),
            "queries" => {
                if matches!(value, WpValue::Null)
                    || matches!(value, WpValue::Array(a) if a.is_empty())
                {
                    self.modify(|s| s.queries.clear());
                }
            }
            "error" => {
                let v = text(value);
                self.modify(|s| {
                    s.error = if v.is_empty() {
                        None
                    } else {
                        Some((String::new(), v))
                    }
                })
            }
            "last_result" => {
                if matches!(value, WpValue::Null)
                    || matches!(value, WpValue::Array(a) if a.is_empty())
                {
                    self.modify(|s| {
                        s.last_result = Rc::new(Vec::new());
                        s.result_generation += 1;
                    });
                }
            }
            "reconnect_retries" => {
                let delay = self.settings.borrow().reconnect_delay;
                self.set_reconnect(to_int(value).max(0) as u32, delay)
            }
            "incompatible_modes" => {
                self.settings.borrow_mut().incompatible_modes = string_list(value)
            }
            "field_types" => {
                let types: IndexMap<String, String> = match value {
                    WpValue::Array(items) => {
                        items.iter().map(|(k, v)| (key_text(k), text(v))).collect()
                    }
                    _ => IndexMap::new(),
                };
                self.set_field_types(types)
            }
            "charset" => self.set_charset_value(text(value)),
            "collate" => self.set_collate(text(value)),
            "prefix" => {
                let v = text(value);
                self.with_tables_mut(|t| t.set_prefix_value(v))
            }
            "base_prefix" => {
                let v = (!matches!(value, WpValue::Null)).then(|| text(value));
                self.with_tables_mut(|t| t.set_base_prefix(v))
            }
            "blogid" => self.with_tables_mut(|t| t.set_blogid(to_int(value))),
            "siteid" => self.with_tables_mut(|t| t.set_siteid(to_int(value))),
            "tables" => self.with_tables_mut(|t| t.set_tables_list(string_list(value))),
            "old_tables" => self.with_tables_mut(|t| t.set_old_tables(string_list(value))),
            "global_tables" => self.with_tables_mut(|t| t.set_global_tables(string_list(value))),
            "ms_global_tables" => {
                self.with_tables_mut(|t| t.set_ms_global_tables(string_list(value)))
            }
            "old_ms_global_tables" => {
                self.with_tables_mut(|t| t.set_old_ms_global_tables(string_list(value)))
            }
            "dbh" => {
                let present = !matches!(value, WpValue::Null | WpValue::Bool(false));
                self.modify(|s| s.dbh_present = present)
            }
            "checking_collation" | "result" | "col_info" | "dbuser" | "dbpassword" | "dbname"
            | "dbhost" | "use_mysqli" | "has_connected" => {}
            "is_mysql" => self.settings.borrow_mut().is_mysql = truthy(value),
            other if TABLE_PROPERTIES.contains(&other) => {
                let v = text(value);
                self.with_tables_mut(|t| t.set(other, v))
            }
            _ => return false,
        }
        true
    }
}

fn key_text(k: &Key) -> String {
    match k {
        Key::Int(i) => i.to_string(),
        Key::Str(s) => s.clone(),
    }
}

fn row_assoc(row: &crate::WpRow) -> WpValue {
    WpValue::Array(
        row.fields
            .iter()
            .map(|(n, v)| {
                (
                    Key::Str(n.clone()),
                    v.clone().map_or(WpValue::Null, string_value),
                )
            })
            .collect(),
    )
}
