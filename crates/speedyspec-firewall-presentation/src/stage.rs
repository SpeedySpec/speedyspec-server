use std::cell::Cell;
use std::net::IpAddr;
use std::rc::Rc;
use std::time::Duration;

use speedyspec_firewall::{FirewallLog, RequestFacts, Verdict};
use speedyspec_firewall_app::FirewallCache;
use speedyspec_native_stage::{NativeResponse, NativeStage, StageOutcome, StageRequest};

/// The client of the request the worker is serving now, so hook callbacks that fire later in the same request
/// (`wp_login_failed`) know who it is. One worker serves one request at a time.
#[derive(Clone, Default)]
pub struct CurrentClient(Rc<Cell<Option<IpAddr>>>);

impl CurrentClient {
    pub fn set(&self, client: IpAddr) {
        self.0.set(Some(client));
    }
    pub fn get(&self) -> Option<IpAddr> {
        self.0.get()
    }
    pub fn clear(&self) {
        self.0.set(None);
    }
}

/// The firewall as a native stage (Adapter). It never blocks because of its own failure.
pub struct FirewallStage {
    cache: Rc<FirewallCache>,
    current: CurrentClient,
    log: Rc<dyn FirewallLog>,
}

impl FirewallStage {
    pub fn new(
        cache: Rc<FirewallCache>,
        current: CurrentClient,
        log: Rc<dyn FirewallLog>,
    ) -> FirewallStage {
        FirewallStage {
            cache,
            current,
            log,
        }
    }
}

impl NativeStage for FirewallStage {
    fn name(&self) -> &str {
        "firewall"
    }

    fn answer(&self, request: &StageRequest<'_>) -> StageOutcome {
        self.current.clear();
        let facts = match RequestFacts::from_params(request.params) {
            Ok(facts) => facts,
            Err(error) => {
                self.log.warn(&format!("request: {error}"));
                return StageOutcome::Continue;
            }
        };
        self.current.set(facts.client);
        let Some(firewall) = self.cache.current() else {
            return StageOutcome::Continue;
        };
        match firewall.check(&facts) {
            Verdict::Block {
                status,
                retry_after,
                ..
            } => StageOutcome::Respond(blocked(status, retry_after)),
            Verdict::Allow | Verdict::Continue => StageOutcome::Continue,
        }
    }
}

fn blocked(status: u16, retry_after: Option<Duration>) -> NativeResponse {
    let mut headers = vec![
        ("Cache-Control".to_string(), "no-store".to_string()),
        (
            "Content-Type".to_string(),
            "text/plain; charset=utf-8".to_string(),
        ),
    ];
    let body = if status == 429 {
        if let Some(after) = retry_after {
            let seconds = (after.as_secs() + u64::from(after.subsec_nanos() > 0)).max(1);
            headers.push(("Retry-After".to_string(), seconds.to_string()));
        }
        b"Too Many Requests\n".to_vec()
    } else {
        b"Forbidden\n".to_vec()
    };
    NativeResponse {
        status,
        headers,
        body,
    }
}
