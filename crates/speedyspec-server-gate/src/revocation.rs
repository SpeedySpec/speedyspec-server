//! Revocation: a sign-out takes effect at once, and every other change of access within one poll of the gate app's
//! revocation feed.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use http::HeaderValue;
use speedyspec_server_domain::AppId;

use crate::calls::{ChangeSet, Changes, GateCalls};
use crate::session::SessionCache;

/// The names of the cookies a `Cookie` header sends.
fn sent_names(cookie: &HeaderValue) -> Vec<String> {
    String::from_utf8_lossy(cookie.as_bytes())
        .split(';')
        .filter_map(|pair| {
            pair.split_once('=')
                .map(|(name, _)| name.trim().to_string())
        })
        .filter(|name| !name.is_empty())
        .collect()
}

/// Whether one `Set-Cookie` value deletes its cookie: `Max-Age` of zero or less, or an `Expires` in the past.
fn deletes(set_cookie: &str) -> Option<String> {
    let mut parts = set_cookie.split(';');
    let name = parts.next()?.split_once('=')?.0.trim().to_string();
    let deleted = parts.map(str::trim).any(|attribute| {
        let Some((key, value)) = attribute.split_once('=') else {
            return false;
        };
        match key.trim().to_ascii_lowercase().as_str() {
            "max-age" => value.trim().parse::<i64>().is_ok_and(|age| age <= 0),
            "expires" => {
                httpdate::parse_http_date(value.trim()).is_ok_and(|at| at <= SystemTime::now())
            }
            _ => false,
        }
    });
    deleted.then_some(name)
}

/// Whether a response clears a cookie its request sent: a sign-out.
pub fn clears_a_sent_cookie(request_cookie: &HeaderValue, set_cookies: &[HeaderValue]) -> bool {
    let sent = sent_names(request_cookie);
    set_cookies
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(deletes)
        .any(|name| sent.contains(&name))
}

/// Where the gate app's revocation feed is, read every round.
pub type ChangesPath = Arc<dyn Fn() -> Option<String> + Send + Sync>;

/// Polls the gate app's revocation feed and forgets the remembered answers it names.
pub struct RevocationPoll;

impl RevocationPoll {
    /// Starts polling the path `path` gives (the gate app's live release's feed) on `app` every `every`, on the
    /// current tokio runtime; a round with no path is skipped. A poll that fails forgets every answer and stops
    /// remembering new ones until a poll succeeds (fail closed).
    pub fn spawn(
        app: AppId,
        path: ChangesPath,
        every: Duration,
        calls: Arc<dyn GateCalls>,
        cache: Arc<SessionCache>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut cursor = String::new();
            let mut ticks = tokio::time::interval(every);
            ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticks.tick().await;
                let Some(path) = path() else {
                    continue;
                };
                match calls.changes(&app, &path, &cursor).await {
                    Ok(Changes {
                        cursor: next,
                        change,
                    }) => {
                        match change {
                            ChangeSet::Users(users) if !users.is_empty() => {
                                cache.forget_users(&users)
                            }
                            ChangeSet::Users(_) => {}
                            ChangeSet::Reset => cache.forget_all(),
                        }
                        cache.set_bypass(false);
                        cursor = next;
                    }
                    Err(err) => {
                        tracing::warn!(event = "gate_changes_unavailable", error = %err);
                        cache.forget_all();
                        cache.set_bypass(true);
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::time::Duration;

    use http::HeaderValue;
    use speedyspec_server_domain::AppId;

    use super::*;
    use crate::calls::{BoxFuture, CallError, GateCalls, SessionAnswer, SiteState};
    use crate::session::{CacheOutcome, SessionCache};

    fn hv(value: &str) -> HeaderValue {
        HeaderValue::from_str(value).unwrap()
    }

    #[test]
    fn a_cleared_sent_cookie_is_noticed() {
        let request = hv("wordpress_logged_in_x=alice");
        assert!(clears_a_sent_cookie(
            &request,
            &[hv("wordpress_logged_in_x=; Max-Age=0; Path=/")]
        ));
        assert!(clears_a_sent_cookie(
            &request,
            &[hv(
                "wordpress_logged_in_x=deleted; expires=Thu, 01 Jan 1970 00:00:01 GMT"
            )]
        ));
        assert!(clears_a_sent_cookie(
            &request,
            &[hv("wordpress_logged_in_x=; max-age=-1")]
        ));
        assert!(
            !clears_a_sent_cookie(&request, &[hv("other=; Max-Age=0")]),
            "not a cookie the request sent"
        );
        assert!(!clears_a_sent_cookie(
            &request,
            &[hv("wordpress_logged_in_x=new; Max-Age=3600")]
        ));
        assert!(!clears_a_sent_cookie(&request, &[]));
    }

    #[test]
    fn signout_clearing_one_of_several_cookies_forgets_the_header() {
        let request = hv("a=1; wordpress_logged_in_x=alice; b=2");
        assert!(clears_a_sent_cookie(
            &request,
            &[hv("theme=dark"), hv("wordpress_logged_in_x=; Max-Age=0")]
        ));
    }

    /// Answers `changes` from a script of answers.
    struct Feed(Mutex<Vec<Result<Changes, CallError>>>, Mutex<Vec<String>>);

    impl GateCalls for Feed {
        fn probe<'a>(&'a self, _: &'a AppId, _: &'a str) -> BoxFuture<'a, Result<u16, CallError>> {
            Box::pin(async { Ok(200) })
        }
        fn site<'a>(
            &'a self,
            _: &'a AppId,
            _: &'a str,
        ) -> BoxFuture<'a, Result<SiteState, CallError>> {
            Box::pin(async {
                Ok(SiteState {
                    installed: true,
                    maintenance: false,
                })
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
            since: &'a str,
        ) -> BoxFuture<'a, Result<Changes, CallError>> {
            self.1.lock().unwrap().push(since.to_string());
            let next = {
                let mut answers = self.0.lock().unwrap();
                if answers.is_empty() {
                    Err(CallError::Timeout)
                } else {
                    answers.remove(0)
                }
            };
            Box::pin(async move { next })
        }
    }

    async fn remembered(cache: &SessionCache, name: &str, user: u64) -> bool {
        let outcome = cache
            .answer(&hv(name), async move {
                Ok(SessionAnswer::SignedIn {
                    user,
                    pages: Default::default(),
                })
            })
            .await;
        assert!(matches!(outcome, CacheOutcome::SignedIn { .. }));
        // A second lookup that would sign out shows whether the first was kept.
        matches!(
            cache
                .answer(&hv(name), async { Ok(SessionAnswer::SignedOut) })
                .await,
            CacheOutcome::SignedIn { .. }
        )
    }

    fn feed(answers: Vec<Result<Changes, CallError>>) -> Arc<Feed> {
        Arc::new(Feed(Mutex::new(answers), Mutex::new(Vec::new())))
    }

    #[tokio::test(start_paused = true)]
    async fn users_are_forgotten_within_one_poll() {
        let cache = Arc::new(SessionCache::new(Duration::from_secs(60), 16));
        assert!(remembered(&cache, "u=5", 5).await && remembered(&cache, "u=6", 6).await);
        let calls = feed(vec![
            Ok(Changes {
                cursor: "1".into(),
                change: ChangeSet::Users(vec![]),
            }),
            Ok(Changes {
                cursor: "2".into(),
                change: ChangeSet::Users(vec![5]),
            }),
        ]);
        let poll = RevocationPoll::spawn(
            AppId::parse("admin-api").unwrap(),
            Arc::new(|| Some("/changes".to_string())),
            Duration::from_secs(1),
            calls.clone(),
            cache.clone(),
        );
        tokio::time::sleep(Duration::from_millis(1500)).await;
        assert_eq!(
            calls.1.lock().unwrap().as_slice(),
            ["", "1"],
            "the cursor is carried"
        );
        // u=5 was forgotten, so its next lookup asks again (and is signed out); u=6 is still remembered.
        assert!(matches!(
            cache
                .answer(&hv("u=5"), async { Ok(SessionAnswer::SignedOut) })
                .await,
            CacheOutcome::SignedOut
        ));
        assert!(matches!(
            cache
                .answer(&hv("u=6"), async { Ok(SessionAnswer::SignedOut) })
                .await,
            CacheOutcome::SignedIn { .. }
        ));
        poll.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn reset_forgets_everything() {
        let cache = Arc::new(SessionCache::new(Duration::from_secs(60), 16));
        assert!(remembered(&cache, "u=6", 6).await);
        let calls = feed(vec![Ok(Changes {
            cursor: "9".into(),
            change: ChangeSet::Reset,
        })]);
        let poll = RevocationPoll::spawn(
            AppId::parse("admin-api").unwrap(),
            Arc::new(|| Some("/changes".to_string())),
            Duration::from_secs(1),
            calls,
            cache.clone(),
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(matches!(
            cache
                .answer(&hv("u=6"), async { Ok(SessionAnswer::SignedOut) })
                .await,
            CacheOutcome::SignedOut
        ));
        poll.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_poll_bypasses_until_success() {
        let cache = Arc::new(SessionCache::new(Duration::from_secs(60), 16));
        let calls = feed(vec![
            Err(CallError::Timeout),
            Ok(Changes {
                cursor: "1".into(),
                change: ChangeSet::Users(vec![]),
            }),
        ]);
        let poll = RevocationPoll::spawn(
            AppId::parse("admin-api").unwrap(),
            Arc::new(|| Some("/changes".to_string())),
            Duration::from_secs(1),
            calls,
            cache.clone(),
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            !remembered(&cache, "u=7", 7).await,
            "while the feed fails, nothing is remembered"
        );
        tokio::time::sleep(Duration::from_millis(1100)).await;
        assert!(
            remembered(&cache, "u=7", 7).await,
            "once it answers, answers are remembered again"
        );
        poll.abort();
    }
}
