//! The admin gate: an [`AccessCheck`] that applies the live release's access rules, then asks the gate app who the
//! visitor is and which pages they may load.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use http::HeaderValue;
use speedyspec_server_domain::{
    AccessCheck, AccessDecision, AccessRequest, AppId, GateSection, Identity,
};
use tokio::time::Instant;

use crate::calls::{CallError, GateCalls, SiteState};
use crate::maintenance::MaintenanceSet;
use crate::rules::{AccessRules, RuleOutcome};
use crate::session::{CacheOutcome, SessionCache};

/// The gate contract version this server speaks.
pub const GATE_VERSION: u32 = 1;

/// What the gate reads of the live releases.
pub trait GateView: Send + Sync {
    /// The gate app's endpoints, when it has a live release that names them.
    fn gate(&self) -> Option<GateSection>;
    /// `app`'s live release's access rules, when it has a live release.
    fn rules(&self, app: &AppId) -> Option<Arc<AccessRules>>;
}

/// `[gate]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GateSettings {
    pub app: AppId,
    pub session_ttl: Duration,
    pub timeout: Duration,
    pub site_ttl: Duration,
    pub poll: Duration,
    pub max_pending: usize,
    /// The apps the gate app's site maintenance applies to; `None` is every app.
    pub maintenance_apps: Option<Vec<AppId>>,
}

/// See the module docs.
pub struct GateAccess {
    settings: GateSettings,
    view: Arc<dyn GateView>,
    calls: Arc<dyn GateCalls>,
    maintenance: Arc<MaintenanceSet>,
    sessions: Arc<SessionCache>,
    site: Mutex<Option<(SiteState, Instant)>>,
}

/// A site state that cannot be read fails closed for install: the install page stays off.
const UNKNOWN_SITE: SiteState = SiteState {
    installed: true,
    maintenance: false,
};

impl GateAccess {
    pub fn new(
        settings: GateSettings,
        view: Arc<dyn GateView>,
        calls: Arc<dyn GateCalls>,
        maintenance: Arc<MaintenanceSet>,
    ) -> GateAccess {
        let sessions = Arc::new(SessionCache::new(
            settings.session_ttl,
            settings.max_pending,
        ));
        GateAccess {
            settings,
            view,
            calls,
            maintenance,
            sessions,
            site: Mutex::new(None),
        }
    }

    /// The remembered session answers (revocation forgets them).
    pub fn sessions(&self) -> &Arc<SessionCache> {
        &self.sessions
    }

    /// The gate app's site state, remembered for `site_ttl`.
    pub async fn site_state(&self, gate: &GateSection) -> SiteState {
        if let Some((state, at)) = *self.site.lock().unwrap()
            && at.elapsed() < self.settings.site_ttl
        {
            return state;
        }
        let asked = tokio::time::timeout(
            self.settings.timeout,
            self.calls.site(&self.settings.app, &gate.site),
        )
        .await;
        let state = match asked {
            Ok(Ok(state)) => state,
            Ok(Err(err)) => {
                tracing::warn!(event = "gate_site_unavailable", error = %err);
                UNKNOWN_SITE
            }
            Err(_) => {
                tracing::warn!(event = "gate_site_unavailable", error = %CallError::Timeout);
                UNKNOWN_SITE
            }
        };
        *self.site.lock().unwrap() = Some((state, Instant::now()));
        state
    }

    async fn decide(&self, request: AccessRequest<'_>) -> AccessDecision {
        let (Some(gate), Some(rules)) = (self.view.gate(), self.view.rules(request.app)) else {
            return AccessDecision::Denied;
        };
        let site = self.site_state(&gate).await;
        let outcome = rules.decide(request.rel_path, site, self.maintenance.is_on(request.app));
        let page = match outcome {
            RuleOutcome::Public => return AccessDecision::Allowed(Identity::anonymous()),
            RuleOutcome::Disabled => return AccessDecision::Disabled,
            RuleOutcome::Page(page) => Some(page),
            RuleOutcome::SignedIn => None,
        };
        let Some(cookie) = cookie_header(request.headers) else {
            return AccessDecision::Denied;
        };
        let ask = async {
            match tokio::time::timeout(
                self.settings.timeout,
                self.calls
                    .session(&self.settings.app, &gate.session, &cookie),
            )
            .await
            {
                Ok(answer) => answer,
                Err(_) => Err(CallError::Timeout),
            }
        };
        match self.sessions.answer(&cookie, ask).await {
            CacheOutcome::SignedOut => AccessDecision::Denied,
            CacheOutcome::Busy => AccessDecision::Busy,
            CacheOutcome::SignedIn { user, pages } => match page {
                Some(page) if !pages.contains(&page) => AccessDecision::Forbidden,
                _ => AccessDecision::Allowed(Identity {
                    user_id: Some(user),
                }),
            },
        }
    }
}

/// The request's cookies as one header value (HTTP/2 may send several `Cookie` headers).
pub fn cookie_header(headers: &http::HeaderMap) -> Option<HeaderValue> {
    let values: Vec<&[u8]> = headers
        .get_all(http::header::COOKIE)
        .iter()
        .map(HeaderValue::as_bytes)
        .filter(|value| !value.is_empty())
        .collect();
    match values.as_slice() {
        [] => None,
        [one] => HeaderValue::from_bytes(one).ok(),
        many => HeaderValue::from_bytes(&many.join(&b"; "[..])).ok(),
    }
}

impl AccessCheck for GateAccess {
    fn check<'a>(
        &'a self,
        request: AccessRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = AccessDecision> + Send + 'a>> {
        Box::pin(self.decide(request))
    }
}
