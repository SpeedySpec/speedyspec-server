//! The timing Decorator over the `default` SQL store.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use speedyspec_data::{
    Capabilities, DbError, Dialect, ExecResult, Introspector, Prepared, Rows, SqlStore, Statement,
};

/// `microtime(true)`.
pub fn microtime() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// When the last statement started, how long it took, and (when capturing) what the engine ran.
#[derive(Debug, Clone, PartialEq)]
pub struct Timing {
    pub start: f64,
    pub elapsed: f64,
    pub emitted: Vec<String>,
    pub engine: String,
}

/// Times every statement; captures the emitted SQL only when asked (`SAVEQUERIES`).
pub struct TimedSqlStore {
    inner: Rc<dyn SqlStore>,
    capture: Cell<bool>,
    timing: RefCell<Option<Timing>>,
}

impl TimedSqlStore {
    pub fn new(inner: Rc<dyn SqlStore>) -> TimedSqlStore {
        TimedSqlStore {
            inner,
            capture: Cell::new(false),
            timing: RefCell::new(None),
        }
    }

    pub fn capture_emitted(&self, on: bool) {
        self.capture.set(on);
    }

    pub fn take_timing(&self) -> Option<Timing> {
        self.timing.borrow_mut().take()
    }

    fn timed<T>(
        &self,
        stmt: &Statement,
        run: impl FnOnce() -> Result<T, DbError>,
    ) -> Result<T, DbError> {
        let start = microtime();
        let result = run();
        let elapsed = (microtime() - start).max(0.0);
        let emitted = if self.capture.get() {
            match self.inner.dialect().prepare(stmt) {
                Ok(Prepared::Sql(sql)) => sql.into_iter().map(|e| e.sql).collect(),
                Ok(Prepared::Emulated(op)) => vec![format!("{op:?}")],
                Err(_) => Vec::new(),
            }
        } else {
            Vec::new()
        };
        *self.timing.borrow_mut() = Some(Timing {
            start,
            elapsed,
            emitted,
            engine: self.inner.engine().to_string(),
        });
        result
    }
}

impl SqlStore for TimedSqlStore {
    fn engine(&self) -> &str {
        self.inner.engine()
    }
    fn dialect(&self) -> &dyn Dialect {
        self.inner.dialect()
    }
    fn capabilities(&self) -> Capabilities {
        self.inner.capabilities()
    }
    fn execute(&self, stmt: &Statement) -> Result<ExecResult, DbError> {
        self.timed(stmt, || self.inner.execute(stmt))
    }
    fn query(&self, stmt: &Statement) -> Result<Rows, DbError> {
        self.timed(stmt, || self.inner.query(stmt))
    }
    fn begin(&self) -> Result<(), DbError> {
        self.inner.begin()
    }
    fn commit(&self) -> Result<(), DbError> {
        self.inner.commit()
    }
    fn rollback(&self) -> Result<(), DbError> {
        self.inner.rollback()
    }
    fn savepoint(&self, name: &str) -> Result<(), DbError> {
        self.inner.savepoint(name)
    }
    fn release_savepoint(&self, name: &str) -> Result<(), DbError> {
        self.inner.release_savepoint(name)
    }
    fn rollback_to_savepoint(&self, name: &str) -> Result<(), DbError> {
        self.inner.rollback_to_savepoint(name)
    }
    fn in_transaction(&self) -> bool {
        self.inner.in_transaction()
    }
    fn introspect(&self) -> &dyn Introspector {
        self.inner.introspect()
    }
    fn reset(&self) -> Result<(), DbError> {
        self.inner.reset()
    }
}
