//! Shared by the entity tests: finds `compat/posts-entity/cases.json` and replays a `dates` case's clock.

use std::path::PathBuf;

use serde_json::Value;
use speedyspec_php_serialize::PhpValue;
use speedyspec_wp_posts_contracts::Clock;

use super::decode;

/// The recorded entity cases. The file is large: read it once per test.
#[allow(dead_code)]
pub fn entity_cases() -> Value {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../compat/posts-entity/cases.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// A `dates` case's `clock`: the recorded answers of the time functions, and nothing else. It panics when asked
/// for now in a case that recorded `null` (the call should not have read the clock), for a date it was not given,
/// or for a timestamp, which the recording does not hold.
#[allow(dead_code)]
pub struct TableClock {
    now_local: Option<String>,
    now_gmt: Option<String>,
    date_from_gmt: Vec<(Vec<u8>, Vec<u8>)>,
    gmt_from_date: Vec<(Vec<u8>, Vec<u8>)>,
}

#[allow(dead_code)]
impl TableClock {
    /// The clock a case recorded.
    pub fn from_case(clock: &Value) -> Self {
        let now = |name: &str| clock[name].as_str().map(str::to_owned);
        Self {
            now_local: now("now_local"),
            now_gmt: now("now_gmt"),
            date_from_gmt: pairs(&clock["date_from_gmt"]),
            gmt_from_date: pairs(&clock["gmt_from_date"]),
        }
    }

    /// The recorded `get_gmt_from_date()` pairs, `(local, gmt)`.
    pub fn gmt_from_date_pairs(&self) -> &[(Vec<u8>, Vec<u8>)] {
        &self.gmt_from_date
    }
}

/// Decodes `[[in, out]]` pairs of encoded strings.
#[allow(dead_code)]
fn pairs(value: &Value) -> Vec<(Vec<u8>, Vec<u8>)> {
    let bytes = |v: &Value| match decode(v) {
        PhpValue::Str(s) => s,
        other => panic!("a recorded date is a string, not {other:?}"),
    };
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|pair| (bytes(&pair[0]), bytes(&pair[1])))
        .collect()
}

/// The answer recorded for `input`.
#[allow(dead_code)]
fn lookup(table: &[(Vec<u8>, Vec<u8>)], function: &str, input: &[u8]) -> Vec<u8> {
    table
        .iter()
        .find(|(given, _)| given.as_slice() == input)
        .map(|(_, out)| out.clone())
        .unwrap_or_else(|| {
            panic!(
                "{function}({:?}) was not recorded",
                String::from_utf8_lossy(input)
            )
        })
}

impl Clock for TableClock {
    fn now_local(&self) -> String {
        self.now_local
            .clone()
            .expect("current_time( 'mysql' ) read in a case that recorded no now")
    }

    fn now_gmt(&self) -> String {
        self.now_gmt
            .clone()
            .expect("current_time( 'mysql', true ) read in a case that recorded no now")
    }

    fn date_from_gmt(&self, gmt: &[u8]) -> Vec<u8> {
        lookup(&self.date_from_gmt, "get_date_from_gmt", gmt)
    }

    fn gmt_from_date(&self, local: &[u8]) -> Vec<u8> {
        lookup(&self.gmt_from_date, "get_gmt_from_date", local)
    }

    fn timestamp(&self, datetime: &[u8]) -> Option<i64> {
        panic!(
            "strtotime({:?}) is not recorded",
            String::from_utf8_lossy(datetime)
        )
    }
}
