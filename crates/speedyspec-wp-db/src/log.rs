//! The `SAVEQUERIES` query log and `log_query_custom_data` (a Decorator step after each statement runs).

use speedyspec_data::Statement;
use speedyspec_wp_hooks_contracts::{Count, HookName};

use crate::php::to_bytes;
use crate::{LoggedQuery, WpDb};
use speedyspec_wp_db_contracts::{WpDbError, WpValue};

fn truthy(v: &WpValue) -> bool {
    match v {
        WpValue::Null | WpValue::Bool(false) | WpValue::Int(0) => false,
        WpValue::Float(f) => *f != 0.0,
        WpValue::Array(a) => !a.is_empty(),
        other => to_bytes(other).is_some_and(|b| !b.is_empty() && b != b"0"),
    }
}

impl WpDb {
    /// `defined('SAVEQUERIES') && SAVEQUERIES`.
    pub(crate) fn save_queries(&self) -> bool {
        self.host
            .constant("SAVEQUERIES")
            .is_some_and(|v| truthy(&v))
    }

    /// Runs before each statement: emission is captured only when it will be logged.
    pub(crate) fn before_run(&self) -> bool {
        let saving = self.save_queries();
        self.timed().capture_emitted(saving);
        saving
    }

    /// Runs after each statement: with `SAVEQUERIES`, logs it as `_do_query` does.
    pub(crate) fn after_run(&self, _stmt: &Statement, saving: bool) {
        let timing = self.timed().take_timing();
        if !saving {
            return;
        }
        let Some(timing) = timing else { return };
        self.modify(|s| s.time_start = Some(timing.start));
        let query = self.last_query_text();
        let caller = self.host.caller();
        self.append_log(
            &query,
            timing.elapsed,
            &caller,
            timing.start,
            WpValue::Array(Vec::new()),
            timing.emitted,
            timing.engine,
        );
    }

    /// `wpdb::log_query`.
    pub fn log_query(&self, query: &str, time: f64, caller: &str, start: f64, data: WpValue) {
        self.append_log(query, time, caller, start, data, Vec::new(), String::new());
    }

    #[allow(clippy::too_many_arguments)]
    fn append_log(
        &self,
        query: &str,
        time: f64,
        caller: &str,
        start: f64,
        data: WpValue,
        emitted: Vec<String>,
        engine: String,
    ) {
        let hook = HookName::from("log_query_custom_data");
        let data = match self.hooks.skip_if_empty(&hook, Count::Filters) {
            Ok(true) => data,
            _ => self
                .hooks
                .apply_filters(
                    &hook,
                    data.clone(),
                    vec![
                        WpValue::Str(query.into()),
                        WpValue::Float(time),
                        WpValue::Str(caller.into()),
                        WpValue::Float(start),
                    ],
                )
                .map(|v| v.read().into_owned())
                .unwrap_or_else(|e| {
                    // Recorded for the bridge to rethrow; the entry keeps its data.
                    let _ = WpDbError::from(e);
                    data
                }),
        };
        let entry = LoggedQuery {
            query: query.into(),
            time,
            caller: caller.into(),
            start,
            data,
            emitted,
            engine,
        };
        self.modify(|s| s.queries.push(entry));
    }

    /// `$wpdb->queries`, with the engine's view of each.
    pub fn query_log(&self) -> Vec<LoggedQuery> {
        self.read(|s| s.queries.clone())
    }
}
