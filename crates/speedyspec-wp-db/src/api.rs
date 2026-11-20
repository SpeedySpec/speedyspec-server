//! `WpDb`: the port's API (Gateway), and the query path.

use std::cell::RefCell;
use std::rc::Rc;

use indexmap::IndexMap;
use speedyspec_data::{DbError, SqlStore, Statement, StatementKind};
use speedyspec_wp_hooks::{Count, HookApi, HookError, HookName};

use crate::binary::parse_bytes;
use crate::escape::string_value;
use crate::php::to_bytes;
use crate::timed::microtime;
use crate::{
    LastQuery, PlaceholderEscape, QueryClass, RequestState, ServerCaps, TableEnv, TableNames,
    TimedSqlStore, WpDbConfig, WpDbError, WpRow, WpValue, WpdbHost, WpdbState,
};

/// What `WpDb` is made from.
pub struct WpDbParts {
    pub store: Rc<dyn SqlStore>,
    pub hooks: Rc<HookApi>,
    pub state: Rc<dyn WpdbState>,
    pub host: Rc<dyn WpdbHost>,
    pub config: WpDbConfig,
}

/// What `query()` returns: rows for a read, counts for a write, `true` for DDL.
#[derive(Debug, Clone, PartialEq)]
pub enum QueryOutcome {
    Rows(Rc<Vec<WpRow>>),
    Affected { rows: u64, insert_id: u64 },
    Done,
}

impl QueryOutcome {
    /// `query()`'s PHP return value.
    pub fn to_wp(&self) -> WpValue {
        match self {
            QueryOutcome::Rows(rows) => WpValue::Int(rows.len() as i64),
            QueryOutcome::Affected { rows, .. } => WpValue::Int(*rows as i64),
            QueryOutcome::Done => WpValue::Bool(true),
        }
    }
}

/// `wpdb`'s settings that outlive a request.
pub(crate) struct Settings {
    pub charset: String,
    pub collate: String,
    pub caps: Option<ServerCaps>,
    pub field_types: IndexMap<String, String>,
    pub allow_unsafe_unquoted_parameters: bool,
    pub reconnect_retries: u32,
    pub reconnect_delay: std::time::Duration,
    pub has_connected: bool,
    pub incompatible_modes: Vec<String>,
}

/// WordPress's `wpdb`.
pub struct WpDb {
    pub(crate) store: Rc<TimedSqlStore>,
    pub(crate) hooks: Rc<HookApi>,
    pub(crate) state: Rc<dyn WpdbState>,
    pub(crate) host: Rc<dyn WpdbHost>,
    pub(crate) config: WpDbConfig,
    pub(crate) escape: PlaceholderEscape,
    pub(crate) tables: RefCell<TableNames>,
    pub(crate) settings: RefCell<Settings>,
    query_hook: HookName,
    statement_hook: HookName,
}

/// `htmlspecialchars($s, ENT_QUOTES)`.
pub(crate) fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#039;"),
            other => out.push(other),
        }
    }
    out
}

/// PHP's `! $value` for a query string.
fn falsy(bytes: &[u8]) -> bool {
    bytes.is_empty() || bytes == b"0"
}

impl WpDb {
    pub fn new(parts: WpDbParts) -> WpDb {
        let salt = parts
            .host
            .constant("AUTH_SALT")
            .and_then(|v| to_bytes(&v))
            .map(|b| String::from_utf8_lossy(&b).into_owned());
        let escape = PlaceholderEscape::new(salt.as_deref());
        parts.state.modify(&mut |s| {
            s.ready = true;
            s.dbh_present = true;
        });
        WpDb {
            store: Rc::new(TimedSqlStore::new(parts.store)),
            hooks: parts.hooks,
            state: parts.state,
            host: parts.host,
            config: parts.config,
            escape,
            tables: RefCell::new(TableNames::new()),
            settings: RefCell::new(Settings::fresh()),
            query_hook: HookName::from("query"),
            statement_hook: HookName::from("speedyspec_query_statement"),
        }
    }
}

impl Settings {
    /// What a new `wpdb` object starts with.
    fn fresh() -> Settings {
        Settings {
            charset: String::new(),
            collate: String::new(),
            caps: None,
            field_types: IndexMap::new(),
            allow_unsafe_unquoted_parameters: true,
            reconnect_retries: 5,
            reconnect_delay: std::time::Duration::from_secs(1),
            has_connected: false,
            incompatible_modes: [
                "NO_ZERO_DATE",
                "ONLY_FULL_GROUP_BY",
                "STRICT_TRANS_TABLES",
                "STRICT_ALL_TABLES",
                "TRADITIONAL",
                "ANSI",
            ]
            .iter()
            .map(|m| m.to_string())
            .collect(),
        }
    }
}

impl WpDb {
    /// Starts a request as a new `wpdb` object would: table names, prefixes, field types and the charset settings
    /// back to their defaults. The server's capabilities, read once, are kept.
    pub fn reset_request(&self) {
        let caps = self.settings.borrow().caps.clone();
        *self.settings.borrow_mut() = Settings {
            caps,
            ..Settings::fresh()
        };
        *self.tables.borrow_mut() = TableNames::new();
    }

    /// The timed store, for Rust callers sharing the connection.
    pub fn store(&self) -> Rc<dyn SqlStore> {
        self.store.clone()
    }

    pub(crate) fn timed(&self) -> &TimedSqlStore {
        &self.store
    }

    pub fn state(&self) -> &Rc<dyn WpdbState> {
        &self.state
    }

    pub fn host(&self) -> &Rc<dyn WpdbHost> {
        &self.host
    }

    pub fn hooks(&self) -> &Rc<HookApi> {
        &self.hooks
    }

    pub fn config(&self) -> &WpDbConfig {
        &self.config
    }

    pub fn placeholder_escape(&self) -> &PlaceholderEscape {
        &self.escape
    }

    /// The table names, read.
    pub fn with_tables<T>(&self, f: impl FnOnce(&TableNames) -> T) -> T {
        f(&self.tables.borrow())
    }

    /// The table names, changed.
    pub fn with_tables_mut<T>(&self, f: impl FnOnce(&mut TableNames) -> T) -> T {
        f(&mut self.tables.borrow_mut())
    }

    /// What table naming depends on outside `wpdb`.
    pub fn table_env(&self) -> TableEnv {
        let text = |name: &str| {
            self.host
                .constant(name)
                .and_then(|v| to_bytes(&v))
                .map(|b| String::from_utf8_lossy(&b).into_owned())
        };
        TableEnv {
            multisite: self.host.is_multisite(),
            multisite_constant: self.host.constant("MULTISITE").is_some(),
            custom_user_table: text("CUSTOM_USER_TABLE"),
            custom_user_meta_table: text("CUSTOM_USER_META_TABLE"),
        }
    }

    /// The current name of a table (`TableNames::name`), for ported APIs building statements.
    pub fn table_name(&self, logical: &str) -> String {
        self.tables.borrow().name(logical)
    }

    pub(crate) fn read<T>(&self, f: impl FnOnce(&RequestState) -> T) -> T {
        let mut f = Some(f);
        let mut out = None;
        self.state.read(&mut |s| out = f.take().map(|f| f(s)));
        out.expect("read runs its closure")
    }

    pub(crate) fn modify(&self, f: impl FnOnce(&mut RequestState)) {
        let mut f = Some(f);
        self.state.modify(&mut |s| {
            if let Some(f) = f.take() {
                f(s)
            }
        });
    }

    pub(crate) fn last_query_text(&self) -> String {
        self.read(|s| {
            s.last_query
                .as_ref()
                .map(|q| String::from_utf8_lossy(&q.text()).into_owned())
                .unwrap_or_default()
        })
    }

    // ----- the query path

    /// `wpdb::query` for Rust callers.
    pub fn query(&self, sql: &str) -> Result<QueryOutcome, WpDbError> {
        self.query_bytes(sql.as_bytes())
    }

    /// `wpdb::query` for query text that may not be UTF-8 (PHP strings).
    pub fn query_bytes(&self, sql: &[u8]) -> Result<QueryOutcome, WpDbError> {
        if !self.read(|s| s.ready) {
            self.modify(|s| s.check_current_query = true);
            return Err(WpDbError::NotReady);
        }
        let sql = self.filter_query(sql.to_vec())?;
        self.flush();
        let func_call = format!("$db->query(\"{}\")", String::from_utf8_lossy(&sql));
        self.modify(|s| s.func_call = Some(func_call));
        if self.read(|s| s.check_current_query) && !sql.is_ascii() {
            // WordPress compares a WP_Error with the query too, so a failed lookup refuses the query as well.
            let changed = self
                .strip_invalid_text_from_query(&sql)
                .map_or(true, |stripped| stripped != sql);
            self.flush();
            if changed {
                let message = self
                    .host
                    .translate("WordPress database error: Could not perform query because it contains invalid data.");
                let (sql, m) = (sql.clone(), message.clone());
                self.modify(|s| {
                    s.insert_id = 0;
                    s.last_query = Some(LastQuery::Text(sql));
                    s.last_error = m;
                });
                return Err(WpDbError::InvalidData(message));
            }
        }
        let class = QueryClass::of(&sql);
        let text = sql.clone();
        self.modify(|s| {
            s.check_current_query = true;
            s.last_query = Some(LastQuery::Text(text));
        });
        match parse_bytes(&sql) {
            Ok(stmt) => self.execute(stmt, class),
            Err(e) => {
                // WordPress sends the text and MySQL rejects it; it counts as a query.
                self.modify(|s| s.num_queries += 1);
                self.fail(WpDbError::Db(e), class)
            }
        }
    }

    /// The path a ported API's statement takes: the `query` filter's fast path keeps the tree; with a callback on
    /// `query` the statement is rendered, filtered and parsed again.
    pub fn query_statement(&self, stmt: Statement) -> Result<QueryOutcome, WpDbError> {
        if !self.read(|s| s.ready) {
            self.modify(|s| s.check_current_query = true);
            return Err(WpDbError::NotReady);
        }
        let (stmt, last) = if self.hooks.skip_if_empty(&self.query_hook, Count::Filters)? {
            let stmt = Rc::new(stmt);
            ((*stmt).clone(), LastQuery::Statement(stmt))
        } else {
            let text = LastQuery::Statement(Rc::new(stmt)).text();
            let filtered = self.run_query_filter(text)?;
            let class = QueryClass::of(&filtered);
            self.flush();
            self.modify(|s| s.last_query = Some(LastQuery::Text(filtered.clone())));
            match parse_bytes(&filtered) {
                Ok(stmt) => (stmt, LastQuery::Text(filtered)),
                Err(e) => return self.fail(WpDbError::Db(e), class),
            }
        };
        let class = match stmt.kind() {
            StatementKind::Insert | StatementKind::Replace => QueryClass::Write { insert: true },
            StatementKind::Update | StatementKind::Delete => QueryClass::Write { insert: false },
            StatementKind::Ddl => QueryClass::Ddl,
            _ => QueryClass::Read,
        };
        self.flush();
        self.modify(|s| {
            s.func_call = None;
            s.check_current_query = true;
            s.last_query = Some(last);
        });
        self.execute(stmt, class)
    }

    /// The placeholder escape removed (WordPress's priority-0 `remove_placeholder_escape`), then the `query` filter.
    fn filter_query(&self, sql: Vec<u8>) -> Result<Vec<u8>, WpDbError> {
        let sql = self.escape.remove_bytes(&sql);
        if self.hooks.skip_if_empty(&self.query_hook, Count::Filters)? {
            if falsy(&sql) {
                self.modify(|s| s.insert_id = 0);
                return Err(WpDbError::EmptyQuery);
            }
            return Ok(sql);
        }
        self.run_query_filter(sql)
    }

    fn run_query_filter(&self, sql: Vec<u8>) -> Result<Vec<u8>, WpDbError> {
        let filtered = self
            .hooks
            .apply_filters(&self.query_hook, string_value(sql), Vec::new())?;
        let filtered = match &*filtered.read() {
            WpValue::Null | WpValue::Bool(false) => Vec::new(),
            other => to_bytes(other).unwrap_or_else(|| b"Array".to_vec()),
        };
        let filtered = self.escape.remove_bytes(&filtered);
        if falsy(&filtered) {
            self.modify(|s| s.insert_id = 0);
            return Err(WpDbError::EmptyQuery);
        }
        Ok(filtered)
    }

    /// The Rust-only `speedyspec_query_statement` filter.
    fn filter_statement(&self, stmt: Statement) -> Result<Statement, WpDbError> {
        if self
            .hooks
            .skip_if_empty(&self.statement_hook, Count::Filters)?
        {
            return Ok(stmt);
        }
        let filtered = self.hooks.apply_filters(
            &self.statement_hook,
            WpValue::Opaque(Rc::new(stmt)),
            Vec::new(),
        )?;
        match &filtered {
            WpValue::Opaque(any) => any.downcast_ref::<Statement>().cloned().ok_or_else(|| {
                WpDbError::Hook(HookError::message(
                    "speedyspec_query_statement must return a statement",
                ))
            }),
            _ => Err(WpDbError::Hook(HookError::message(
                "speedyspec_query_statement must return a statement",
            ))),
        }
    }

    fn run(&self, stmt: &Statement) -> Result<RunResult, DbError> {
        let saving = self.before_run();
        let result = match stmt.kind() {
            StatementKind::Select | StatementKind::Show | StatementKind::Other => {
                self.store.query(stmt).map(RunResult::Rows)
            }
            _ => self.store.execute(stmt).map(RunResult::Exec),
        };
        self.modify(|s| s.num_queries += 1);
        self.after_run(stmt, saving);
        result
    }

    fn execute(&self, stmt: Statement, class: QueryClass) -> Result<QueryOutcome, WpDbError> {
        let stmt = self.filter_statement(stmt)?;
        let mut result = self.run(&stmt);
        if matches!(result, Err(DbError::Connection { .. })) {
            result = self.run(&stmt);
            if matches!(result, Err(DbError::Connection { .. })) {
                if self.check_connection(true).unwrap_or(false) {
                    result = self.run(&stmt);
                } else {
                    self.modify(|s| s.insert_id = 0);
                    return Err(WpDbError::Db(result.err().expect("checked above")));
                }
            }
        }
        let result = match result {
            Ok(r) => r,
            Err(e) => return self.fail(WpDbError::Db(e), class),
        };
        Ok(match class {
            QueryClass::Ddl => QueryOutcome::Done,
            QueryClass::Write { insert } => {
                let (rows, id) = match result {
                    RunResult::Exec(e) => (e.rows_affected, e.insert_id),
                    RunResult::Rows(_) => (0, None),
                };
                let mut insert_id = 0;
                self.modify(|s| {
                    s.rows_affected = rows;
                    if insert {
                        s.insert_id = id.unwrap_or(0);
                    }
                    insert_id = s.insert_id;
                });
                QueryOutcome::Affected { rows, insert_id }
            }
            QueryClass::Read => {
                let (rows, columns) = match result {
                    RunResult::Rows(r) => (
                        r.rows.iter().map(WpRow::from_row).collect::<Vec<_>>(),
                        r.columns,
                    ),
                    RunResult::Exec(_) => (Vec::new(), Vec::new()),
                };
                let rows = Rc::new(rows);
                let shared = rows.clone();
                self.modify(|s| {
                    s.num_rows = shared.len() as u64;
                    s.last_result = shared;
                    s.col_info = Some(Rc::new(columns));
                    s.result_generation += 1;
                });
                QueryOutcome::Rows(rows)
            }
        })
    }

    fn fail(&self, error: WpDbError, class: QueryClass) -> Result<QueryOutcome, WpDbError> {
        let message = error.last_error();
        self.modify(|s| {
            s.last_error = message;
            if s.insert_id != 0 && matches!(class, QueryClass::Write { insert: true }) {
                s.insert_id = 0;
            }
        });
        self.print_error(None);
        Err(error)
    }

    /// `wpdb::flush`.
    pub fn flush(&self) {
        self.modify(|s| {
            s.last_result = Rc::new(Vec::new());
            s.result_generation += 1;
            s.col_info = None;
            s.last_query = None;
            s.rows_affected = 0;
            s.num_rows = 0;
            s.last_error.clear();
        });
    }

    /// `wpdb::print_error`.
    pub fn print_error(&self, message: Option<&str>) {
        let error = match message.filter(|m| !m.is_empty()) {
            Some(m) => m.to_string(),
            None => self.read(|s| s.last_error.clone()),
        };
        let query = self.last_query_text();
        self.host.record_error(&query, &error);
        if self.read(|s| s.suppress_errors) {
            return;
        }
        let caller = self.host.caller();
        let line = if caller.is_empty() {
            format!("WordPress database error {error} for query {query}")
        } else {
            format!("WordPress database error {error} for query {query} made by {caller}")
        };
        self.host.error_log(&line, None);
        if !self.read(|s| s.show_errors) {
            return;
        }
        let label = self.host.translate("WordPress database error:");
        if self.host.is_multisite() {
            let msg = format!("{label} [{error}]\n{query}\n");
            if let Some(file) = self
                .host
                .constant("ERRORLOGFILE")
                .and_then(|v| to_bytes(&v))
            {
                self.host
                    .error_log(&msg, Some(&String::from_utf8_lossy(&file)));
            }
            if self.host.constant("DIEONDBERROR").is_some() {
                self.host.die(&msg);
            }
        } else {
            self.host.output(&format!(
                "<div id=\"error\"><p class=\"wpdberror\"><strong>{label}</strong> [{}]<br /><code>{}</code></p></div>",
                html_escape(&error),
                html_escape(&query)
            ));
        }
    }

    /// `wpdb::show_errors`; returns the old setting.
    pub fn show_errors(&self, show: bool) -> bool {
        let mut old = false;
        self.modify(|s| {
            old = s.show_errors;
            s.show_errors = show;
        });
        old
    }

    pub fn hide_errors(&self) -> bool {
        self.show_errors(false)
    }

    pub fn suppress_errors(&self, suppress: bool) -> bool {
        let mut old = false;
        self.modify(|s| {
            old = s.suppress_errors;
            s.suppress_errors = suppress;
        });
        old
    }

    /// `wpdb::bail`: with `show_errors` the page dies; otherwise the error is kept and false returned.
    pub fn bail(&self, message: &str, code: &str) -> bool {
        if self.read(|s| s.show_errors) {
            let error = self.read(|s| s.last_error.clone());
            let message = if error.is_empty() {
                message.to_string()
            } else {
                format!("<p><code>{error}</code></p>\n{message}")
            };
            self.host.die(&message);
        } else {
            let (code, message) = (code.to_string(), message.to_string());
            self.modify(|s| s.error = Some((code, message)));
        }
        false
    }

    pub fn timer_start(&self) -> bool {
        let now = microtime();
        self.modify(|s| s.time_start = Some(now));
        true
    }

    pub fn timer_stop(&self) -> f64 {
        microtime() - self.read(|s| s.time_start.unwrap_or(0.0))
    }
}

pub(crate) enum RunResult {
    Rows(speedyspec_data::Rows),
    Exec(speedyspec_data::ExecResult),
}
