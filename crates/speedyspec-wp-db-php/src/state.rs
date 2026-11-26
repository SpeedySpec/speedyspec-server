//! The request state, resolved through the container so a PHP state bound under `WPDB_STATE` replaces the memory one.

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_wp_db::{MemoryWpdbState, RequestState, WPDB_STATE, WpdbState};

/// The `WpdbState` bound in the container now, else the worker's memory state.
pub struct ResolvedWpdbState {
    container: Rc<Container>,
    memory: Rc<MemoryWpdbState>,
}

impl ResolvedWpdbState {
    pub fn new(container: Rc<Container>, memory: Rc<MemoryWpdbState>) -> ResolvedWpdbState {
        ResolvedWpdbState { container, memory }
    }

    fn current(&self) -> Rc<dyn WpdbState> {
        self.container
            .make_port::<dyn WpdbState>(WPDB_STATE)
            .unwrap_or_else(|_| self.memory.clone())
    }
}

impl WpdbState for ResolvedWpdbState {
    fn read(&self, f: &mut dyn FnMut(&RequestState)) {
        self.current().read(f)
    }
    fn modify(&self, f: &mut dyn FnMut(&mut RequestState)) {
        self.current().modify(f)
    }
    fn reset(&self) {
        self.current().reset()
    }
}

use std::cell::RefCell;

use speedyspec_php_ext::{Key, PhpObject, Value};
use speedyspec_wp_db::LastQuery;
use speedyspec_wp_db_contracts::WpRow;

use crate::convert::{bytes, bytes_value, int, key_string, list, opt_bytes, string, truthy};

/// The twin of `WpdbState` in PHP: an object implementing `SpeedySpec\WpDb\WpdbState { load(): array;
/// store(array $state): void; reset(): void; }`. Rows travel as lists of `[name, value]` pairs. The query log,
/// column info and column metadata stay in Rust for the request.
pub struct PhpWpdbState {
    object: PhpObject,
    local: RefCell<RequestState>,
}

impl PhpWpdbState {
    /// A state bound while `wpdb` is connected starts ready, as `WpDb::new` leaves the worker's state.
    pub fn new(object: PhpObject) -> PhpWpdbState {
        let state = RequestState {
            ready: true,
            dbh_present: true,
            ..RequestState::default()
        };
        PhpWpdbState {
            object,
            local: RefCell::new(state),
        }
    }

    fn load(&self) -> RequestState {
        let mut state = self.local.borrow().clone();
        if let Ok(Value::Array(items)) = self.object.call("load", Vec::new())
            && !items.is_empty()
        {
            from_value(&items, &mut state);
        }
        state
    }
}

fn get<'a>(items: &'a [(Key, Value)], name: &str) -> Option<&'a Value> {
    items
        .iter()
        .find(|(k, _)| key_string(k) == name)
        .map(|(_, v)| v)
}

fn from_value(items: &[(Key, Value)], s: &mut RequestState) {
    let num = |n: &str| get(items, n).map(int).unwrap_or(0).max(0) as u64;
    let flag = |n: &str, d: bool| get(items, n).map(truthy).unwrap_or(d);
    s.last_query = match get(items, "last_query") {
        Some(Value::Null) | None => None,
        Some(v) => bytes(v).map(LastQuery::Text),
    };
    s.last_result = Rc::new(match get(items, "last_result") {
        Some(Value::Array(rows)) => rows
            .iter()
            .map(|(_, row)| WpRow {
                fields: match row {
                    Value::Array(pairs) => pairs
                        .iter()
                        .filter_map(|(_, pair)| match pair {
                            Value::Array(p) if p.len() == 2 => Some((
                                string(&p[0].1),
                                if matches!(p[1].1, Value::Null) {
                                    None
                                } else {
                                    bytes(&p[1].1)
                                },
                            )),
                            _ => None,
                        })
                        .collect(),
                    _ => Vec::new(),
                },
            })
            .collect(),
        _ => Vec::new(),
    });
    s.result_generation = num("result_generation");
    s.last_error = get(items, "last_error").map(string).unwrap_or_default();
    s.num_rows = num("num_rows");
    s.rows_affected = num("rows_affected");
    s.insert_id = num("insert_id");
    s.num_queries = num("num_queries");
    s.func_call = get(items, "func_call")
        .filter(|v| !matches!(v, Value::Null))
        .map(string);
    s.show_errors = flag("show_errors", false);
    s.suppress_errors = flag("suppress_errors", false);
    s.check_current_query = flag("check_current_query", true);
    s.ready = flag("ready", true);
    s.dbh_present = flag("dbh_present", true);
    s.time_start = match get(items, "time_start") {
        Some(Value::Float(f)) => Some(*f),
        Some(Value::Int(i)) => Some(*i as f64),
        _ => None,
    };
    s.error = match get(items, "error") {
        Some(Value::Array(e)) if e.len() == 2 => Some((string(&e[0].1), string(&e[1].1))),
        _ => None,
    };
    s.table_charset = match get(items, "table_charset") {
        Some(Value::Array(t)) => t
            .iter()
            .map(|(k, v)| {
                (
                    key_string(k),
                    if matches!(v, Value::Bool(false)) {
                        None
                    } else {
                        Some(string(v))
                    },
                )
            })
            .collect(),
        _ => Default::default(),
    };
}

fn to_value(s: &RequestState) -> Value {
    let rows = s
        .last_result
        .iter()
        .map(|row| {
            list(
                row.fields
                    .iter()
                    .map(|(n, v)| list(vec![Value::Str(n.clone()), opt_bytes(v.clone())]))
                    .collect(),
            )
        })
        .collect();
    let field = |k: &str, v: Value| (Key::Str(k.into()), v);
    Value::Array(vec![
        field(
            "last_query",
            s.last_query
                .as_ref()
                .map_or(Value::Null, |q| bytes_value(q.text())),
        ),
        field("last_result", list(rows)),
        field("result_generation", Value::Int(s.result_generation as i64)),
        field("last_error", Value::Str(s.last_error.clone())),
        field("num_rows", Value::Int(s.num_rows as i64)),
        field("rows_affected", Value::Int(s.rows_affected as i64)),
        field("insert_id", Value::Int(s.insert_id as i64)),
        field("num_queries", Value::Int(s.num_queries as i64)),
        field(
            "func_call",
            s.func_call.clone().map_or(Value::Null, Value::Str),
        ),
        field("show_errors", Value::Bool(s.show_errors)),
        field("suppress_errors", Value::Bool(s.suppress_errors)),
        field("check_current_query", Value::Bool(s.check_current_query)),
        field("ready", Value::Bool(s.ready)),
        field("dbh_present", Value::Bool(s.dbh_present)),
        field("time_start", s.time_start.map_or(Value::Null, Value::Float)),
        field(
            "error",
            s.error.clone().map_or(Value::Null, |(c, m)| {
                list(vec![Value::Str(c), Value::Str(m)])
            }),
        ),
        field(
            "table_charset",
            Value::Array(
                s.table_charset
                    .iter()
                    .map(|(k, v)| {
                        (
                            Key::Str(k.clone()),
                            v.clone().map_or(Value::Bool(false), Value::Str),
                        )
                    })
                    .collect(),
            ),
        ),
    ])
}

impl WpdbState for PhpWpdbState {
    fn read(&self, f: &mut dyn FnMut(&RequestState)) {
        f(&self.load());
    }

    fn modify(&self, f: &mut dyn FnMut(&mut RequestState)) {
        let mut state = self.load();
        f(&mut state);
        let _ = self.object.call("store", vec![to_value(&state)]);
        *self.local.borrow_mut() = state;
    }

    fn reset(&self) {
        let _ = self.object.call("reset", Vec::new());
        *self.local.borrow_mut() = RequestState {
            ready: true,
            dbh_present: true,
            ..RequestState::default()
        };
    }
}
