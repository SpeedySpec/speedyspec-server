//! The report a worker sends after each request, as an `FCGI_STDERR` record: FastCGI has no per-request trailer.

/// The prefix that marks a stderr record as the report rather than PHP's own output.
pub const REPORT_PREFIX: &[u8] = b"\0speedyspec:";

/// What a worker reports after each request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkerReport {
    /// Peak memory of the request, in bytes.
    pub memory: u64,
    /// Requests this worker has served.
    pub requests: u64,
    /// A fatal error left the engine degraded: the worker exits after this request.
    pub poisoned: bool,
}

impl WorkerReport {
    /// The record's bytes: the prefix, then `memory=<n> requests=<n> poisoned=<0|1>`.
    pub fn encode(&self) -> Vec<u8> {
        let mut record = REPORT_PREFIX.to_vec();
        record.extend_from_slice(
            format!(
                "memory={} requests={} poisoned={}",
                self.memory,
                self.requests,
                u8::from(self.poisoned)
            )
            .as_bytes(),
        );
        record
    }

    /// The report in a stderr record, or `None` when the record is PHP's own output.
    pub fn decode(record: &[u8]) -> Option<WorkerReport> {
        let rest = record.strip_prefix(REPORT_PREFIX)?;
        let text = String::from_utf8_lossy(rest);
        let field = |name: &str| {
            text.split_whitespace()
                .find_map(|kv| kv.strip_prefix(name).and_then(|v| v.strip_prefix('=')))
                .and_then(|v| v.parse::<u64>().ok())
        };
        Some(WorkerReport {
            memory: field("memory").unwrap_or(0),
            requests: field("requests").unwrap_or(0),
            poisoned: field("poisoned") == Some(1),
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_round_trips_and_rejects_other_records() {
        let report = WorkerReport {
            memory: 1234,
            requests: 7,
            poisoned: true,
        };
        assert_eq!(WorkerReport::decode(&report.encode()), Some(report));
        assert_eq!(WorkerReport::decode(b"PHP Warning: x"), None);
    }

    #[test]
    fn report_keeps_its_wire_format() {
        let report = WorkerReport {
            memory: 5,
            requests: 2,
            poisoned: false,
        };
        assert_eq!(
            report.encode(),
            b"\0speedyspec:memory=5 requests=2 poisoned=0".to_vec()
        );
    }
}
