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
    /// Held while the site state is refreshed, so only one refresh runs at a time.
    refreshing: tokio::sync::Mutex<()>,
    /// Every app the site state's maintenance applies to when `maintenance_apps` is not set.
    all_apps: Vec<AppId>,
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
            refreshing: tokio::sync::Mutex::new(()),
            all_apps: Vec::new(),
        }
    }

    /// The configured apps, which the site state's maintenance applies to unless `maintenance_apps` says otherwise.
    pub fn with_apps(mut self, apps: Vec<AppId>) -> GateAccess {
        self.all_apps = apps;
        self
    }

    /// Reads the site state again if it is stale (the poll calls this, so its maintenance applies without visitors).
    pub async fn refresh_site(&self) {
        if let Some(gate) = self.view.gate() {
            self.site_state(&gate).await;
        }
    }

    /// The remembered session answers (revocation forgets them).
    pub fn sessions(&self) -> &Arc<SessionCache> {
        &self.sessions
    }

    /// The gate app's site state, remembered for `site_ttl`. One refresh runs at a time; while it does, every
    /// other lookup gets the remembered state (only a first-ever lookup waits). A failed read fails closed for
    /// install (`installed: true`) and keeps the last known maintenance, so an admin API in trouble mid-update does
    /// not reopen every app.
    pub async fn site_state(&self, gate: &GateSection) -> SiteState {
        if let Some((state, at)) = *self.site.lock().unwrap()
            && at.elapsed() < self.settings.site_ttl
        {
            return state;
        }
        let _refreshing = match self.refreshing.try_lock() {
            Ok(guard) => guard,
            Err(_) => {
                if let Some((state, _)) = *self.site.lock().unwrap() {
                    return state;
                }
                let guard = self.refreshing.lock().await;
                if let Some((state, at)) = *self.site.lock().unwrap()
                    && at.elapsed() < self.settings.site_ttl
                {
                    return state;
                }
                guard
            }
        };
        let asked = tokio::time::timeout(
            self.settings.timeout,
            self.calls.site(&self.settings.app, &gate.site),
        )
        .await;
        let known = self.site.lock().unwrap().map(|(state, _)| state);
        let (state, read) = match asked {
            Ok(Ok(state)) => (state, true),
            failed => {
                let error = match failed {
                    Ok(Err(err)) => err,
                    _ => CallError::Timeout,
                };
                tracing::warn!(event = "gate_site_unavailable", error = %error);
                let maintenance = known.map_or(UNKNOWN_SITE.maintenance, |state| state.maintenance);
                (
                    SiteState {
                        installed: UNKNOWN_SITE.installed,
                        maintenance,
                    },
                    false,
                )
            }
        };
        *self.site.lock().unwrap() = Some((state, Instant::now()));
        if read {
            let apps = self
                .settings
                .maintenance_apps
                .as_ref()
                .unwrap_or(&self.all_apps);
            for app in apps {
                self.maintenance.set(
                    app,
                    crate::maintenance::MaintenanceSource::Site,
                    state.maintenance,
                );
            }
        }
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

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::calls::{BoxFuture, Changes, SessionAnswer};
    use crate::maintenance::MaintenanceSource;

    /// Answers `site` from a script (an `Err` once the script runs out), each after `delay`.
    struct Sites {
        answers: Mutex<VecDeque<Result<SiteState, CallError>>>,
        calls: AtomicUsize,
        delay: Duration,
    }

    impl GateCalls for Sites {
        fn probe<'a>(&'a self, _: &'a AppId, _: &'a str) -> BoxFuture<'a, Result<u16, CallError>> {
            Box::pin(async { Ok(200) })
        }
        fn site<'a>(
            &'a self,
            _: &'a AppId,
            _: &'a str,
        ) -> BoxFuture<'a, Result<SiteState, CallError>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let next = self
                .answers
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Err(CallError::Timeout));
            Box::pin(async move {
                tokio::time::sleep(self.delay).await;
                next
            })
        }
        fn session<'a>(
            &'a self,
            _: &'a AppId,
            _: &'a str,
            _: &'a HeaderValue,
        ) -> BoxFuture<'a, Result<SessionAnswer, CallError>> {
            Box::pin(async { Ok(SessionAnswer::SignedOut) })
        }
        fn changes<'a>(
            &'a self,
            _: &'a AppId,
            _: &'a str,
            _: &'a str,
        ) -> BoxFuture<'a, Result<Changes, CallError>> {
            Box::pin(async { Err(CallError::Timeout) })
        }
    }

    struct NoView;
    impl GateView for NoView {
        fn gate(&self) -> Option<GateSection> {
            None
        }
        fn rules(&self, _: &AppId) -> Option<Arc<AccessRules>> {
            None
        }
    }

    fn gate_section() -> GateSection {
        GateSection {
            version: 1,
            site: "/site".into(),
            session: "/session".into(),
            changes: "/changes".into(),
        }
    }

    fn gate(
        answers: Vec<Result<SiteState, CallError>>,
        delay: Duration,
    ) -> (GateAccess, Arc<Sites>, Arc<MaintenanceSet>) {
        let calls = Arc::new(Sites {
            answers: Mutex::new(answers.into()),
            calls: AtomicUsize::new(0),
            delay,
        });
        let maintenance = Arc::new(MaintenanceSet::default());
        let settings = GateSettings {
            app: AppId::parse("admin-api").unwrap(),
            session_ttl: Duration::from_secs(60),
            timeout: Duration::from_secs(5),
            site_ttl: Duration::from_millis(100),
            poll: Duration::from_secs(1),
            max_pending: 16,
            maintenance_apps: None,
        };
        let gate = GateAccess::new(
            settings,
            Arc::new(NoView),
            calls.clone(),
            maintenance.clone(),
        )
        .with_apps(vec![AppId::parse("blog").unwrap()]);
        (gate, calls, maintenance)
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_site_read_keeps_maintenance() {
        let on = SiteState {
            installed: true,
            maintenance: true,
        };
        let (gate, _, maintenance) = gate(vec![Ok(on), Err(CallError::Timeout)], Duration::ZERO);
        let blog = AppId::parse("blog").unwrap();
        gate.site_state(&gate_section()).await;
        assert!(
            maintenance
                .sources(&blog)
                .contains(&MaintenanceSource::Site)
        );
        tokio::time::advance(Duration::from_millis(200)).await;
        let state = gate.site_state(&gate_section()).await;
        assert!(state.maintenance && state.installed);
        assert!(
            maintenance
                .sources(&blog)
                .contains(&MaintenanceSource::Site),
            "an unreadable site state keeps the last known maintenance"
        );
    }

    #[tokio::test]
    async fn stale_site_state_is_refreshed_once() {
        let off = SiteState {
            installed: true,
            maintenance: false,
        };
        let (gate, calls, _) = gate(vec![Ok(off), Ok(off), Ok(off)], Duration::from_millis(300));
        let gate = Arc::new(gate);
        gate.site_state(&gate_section()).await;
        tokio::time::sleep(Duration::from_millis(150)).await;
        let started = std::time::Instant::now();
        let lookups: Vec<_> = (0..10)
            .map(|_| {
                let gate = gate.clone();
                tokio::spawn(async move { gate.site_state(&gate_section()).await })
            })
            .collect();
        for lookup in lookups {
            lookup.await.unwrap();
        }
        assert_eq!(
            calls.calls.load(Ordering::SeqCst),
            2,
            "one refresh for ten stale lookups"
        );
        assert!(
            started.elapsed() < Duration::from_millis(600),
            "the others got the remembered state meanwhile"
        );
    }
}
