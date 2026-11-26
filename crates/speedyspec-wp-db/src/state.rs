//! The request state port (`WpdbState`): what `wpdb` keeps between calls within one request.

use std::cell::RefCell;
use std::rc::Rc;

use indexmap::IndexMap;
use speedyspec_data::ColumnInfo;

use crate::LastQuery;
use speedyspec_wp_db_contracts::{WpRow, WpValue};

/// One `$wpdb->queries` entry, with the engine's view (`emitted`, `engine`) beside WordPress's five elements.
#[derive(Debug, Clone, PartialEq)]
pub struct LoggedQuery {
    pub query: String,
    pub time: f64,
    pub caller: String,
    pub start: f64,
    pub data: WpValue,
    pub emitted: Vec<String>,
    pub engine: String,
}

/// A column of `SHOW FULL COLUMNS`, as `col_meta` keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnMeta {
    pub field: String,
    pub sql_type: String,
    pub collation: Option<String>,
}

/// `wpdb`'s per-request properties.
#[derive(Debug, Clone, PartialEq)]
pub struct RequestState {
    pub last_query: Option<LastQuery>,
    pub last_result: Rc<Vec<WpRow>>,
    pub result_generation: u64,
    pub col_info: Option<Rc<Vec<ColumnInfo>>>,
    pub last_error: String,
    pub num_rows: u64,
    pub rows_affected: u64,
    pub insert_id: u64,
    pub num_queries: u64,
    pub queries: Vec<LoggedQuery>,
    pub func_call: Option<String>,
    pub show_errors: bool,
    pub suppress_errors: bool,
    pub check_current_query: bool,
    pub error: Option<(String, String)>,
    pub time_start: Option<f64>,
    pub table_charset: IndexMap<String, Option<String>>,
    pub col_meta: IndexMap<String, Vec<ColumnMeta>>,
    pub ready: bool,
    pub dbh_present: bool,
}

impl Default for RequestState {
    fn default() -> Self {
        RequestState {
            last_query: None,
            last_result: Rc::new(Vec::new()),
            result_generation: 0,
            col_info: None,
            last_error: String::new(),
            num_rows: 0,
            rows_affected: 0,
            insert_id: 0,
            num_queries: 0,
            queries: Vec::new(),
            func_call: None,
            show_errors: false,
            suppress_errors: false,
            check_current_query: true,
            error: None,
            time_start: None,
            table_charset: IndexMap::new(),
            col_meta: IndexMap::new(),
            ready: false,
            dbh_present: false,
        }
    }
}

/// The request state's port; it lives for one request.
pub trait WpdbState {
    fn read(&self, f: &mut dyn FnMut(&RequestState));
    fn modify(&self, f: &mut dyn FnMut(&mut RequestState));
    fn reset(&self);
}

/// The container key a PHP state replaces the memory one under.
pub const WPDB_STATE: &str = "SpeedySpec\\WpDb\\WpdbState";

/// The request state in memory.
#[derive(Default)]
pub struct MemoryWpdbState {
    state: RefCell<RequestState>,
}

impl MemoryWpdbState {
    pub fn new() -> MemoryWpdbState {
        MemoryWpdbState::default()
    }
}

impl WpdbState for MemoryWpdbState {
    fn read(&self, f: &mut dyn FnMut(&RequestState)) {
        f(&self.state.borrow());
    }

    fn modify(&self, f: &mut dyn FnMut(&mut RequestState)) {
        f(&mut self.state.borrow_mut());
    }

    fn reset(&self) {
        *self.state.borrow_mut() = RequestState::default();
    }
}
