//! Remembered session answers: only positive ones, keyed by the SHA-256 of the whole `Cookie` header (never the
//! cookie itself), for a time to live. Concurrent asks for one cookie share one call, and uncached asks in flight are
//! capped so a flood of made-up cookies cannot fill the gate app's pool.

use std::collections::{BTreeSet, HashMap};
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use http::HeaderValue;
use sha2::{Digest, Sha256};
use tokio::sync::OnceCell;
use tokio::time::Instant;

use crate::calls::{CallError, SessionAnswer};

/// What a session lookup comes to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CacheOutcome {
    SignedIn {
        user: u64,
        pages: Arc<BTreeSet<String>>,
    },
    SignedOut,
    /// Too many uncached asks are in flight: try again shortly.
    Busy,
}

type Key = [u8; 32];

struct Entry {
    user: u64,
    pages: Arc<BTreeSet<String>>,
    until: Instant,
}

/// See the module docs.
pub struct SessionCache {
    ttl: Duration,
    max_pending: usize,
    entries: Mutex<HashMap<Key, Entry>>,
    pending: Mutex<HashMap<Key, Arc<OnceCell<CacheOutcome>>>>,
}

fn key(cookie: &HeaderValue) -> Key {
    Sha256::digest(cookie.as_bytes()).into()
}

impl SessionCache {
    pub fn new(ttl: Duration, max_pending: usize) -> SessionCache {
        SessionCache {
            ttl,
            max_pending: max_pending.max(1),
            entries: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
        }
    }

    /// The remembered answer for `cookie`, or `ask`'s (shared with every concurrent lookup of the same cookie). A
    /// failed ask is signed out; only signed-in answers are remembered.
    pub async fn answer(
        &self,
        cookie: &HeaderValue,
        ask: impl Future<Output = Result<SessionAnswer, CallError>>,
    ) -> CacheOutcome {
        let key = key(cookie);
        if let Some(entry) = self.entries.lock().unwrap().get(&key)
            && Instant::now() < entry.until
        {
            return CacheOutcome::SignedIn {
                user: entry.user,
                pages: entry.pages.clone(),
            };
        }
        let cell = {
            let mut pending = self.pending.lock().unwrap();
            match pending.get(&key) {
                Some(cell) => cell.clone(),
                None if pending.len() >= self.max_pending => return CacheOutcome::Busy,
                None => {
                    let cell = Arc::new(OnceCell::new());
                    pending.insert(key, cell.clone());
                    cell
                }
            }
        };
        let outcome = cell
            .get_or_init(|| async {
                let outcome = match ask.await {
                    Ok(SessionAnswer::SignedIn { user, pages }) => {
                        let pages = Arc::new(pages);
                        self.entries.lock().unwrap().insert(
                            key,
                            Entry {
                                user,
                                pages: pages.clone(),
                                until: Instant::now() + self.ttl,
                            },
                        );
                        CacheOutcome::SignedIn { user, pages }
                    }
                    Ok(SessionAnswer::SignedOut) | Err(_) => CacheOutcome::SignedOut,
                };
                self.pending.lock().unwrap().remove(&key);
                outcome
            })
            .await
            .clone();
        outcome
    }

    /// Forgets the answer for this `Cookie` header (a sign-out).
    pub fn forget_header(&self, cookie: &HeaderValue) {
        self.entries.lock().unwrap().remove(&key(cookie));
    }

    /// Forgets every answer for these users (their access changed).
    pub fn forget_users(&self, users: &[u64]) {
        self.entries
            .lock()
            .unwrap()
            .retain(|_, entry| !users.contains(&entry.user));
    }

    /// Forgets every answer.
    pub fn forget_all(&self) {
        self.entries.lock().unwrap().clear();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use http::HeaderValue;

    use super::*;

    fn signed_in(user: u64) -> Result<SessionAnswer, CallError> {
        Ok(SessionAnswer::SignedIn {
            user,
            pages: ["core/general".to_string()].into(),
        })
    }

    fn cookie(value: &str) -> HeaderValue {
        HeaderValue::from_str(value).unwrap()
    }

    #[tokio::test]
    async fn only_signed_in_answers_are_kept() {
        let cache = SessionCache::new(Duration::from_secs(60), 16);
        let calls = AtomicUsize::new(0);
        for _ in 0..2 {
            cache
                .answer(&cookie("a=1"), async {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(SessionAnswer::SignedOut)
                })
                .await;
        }
        assert_eq!(calls.load(Ordering::SeqCst), 2, "a no is asked again");
        for _ in 0..2 {
            let outcome = cache
                .answer(&cookie("b=1"), async {
                    calls.fetch_add(1, Ordering::SeqCst);
                    signed_in(5)
                })
                .await;
            assert!(matches!(outcome, CacheOutcome::SignedIn { user: 5, .. }));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 3, "a yes is remembered");
    }

    #[tokio::test]
    async fn concurrent_asks_share_one_call() {
        let cache = Arc::new(SessionCache::new(Duration::from_secs(60), 16));
        let calls = Arc::new(AtomicUsize::new(0));
        let tasks: Vec<_> = (0..10)
            .map(|_| {
                let (cache, calls) = (cache.clone(), calls.clone());
                tokio::spawn(async move {
                    cache
                        .answer(&cookie("c=1"), async move {
                            calls.fetch_add(1, Ordering::SeqCst);
                            tokio::time::sleep(Duration::from_millis(100)).await;
                            signed_in(7)
                        })
                        .await
                })
            })
            .collect();
        for task in tasks {
            assert!(matches!(
                task.await.unwrap(),
                CacheOutcome::SignedIn { user: 7, .. }
            ));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn past_max_pending_is_busy() {
        let cache = Arc::new(SessionCache::new(Duration::from_secs(60), 2));
        let slow = |cache: Arc<SessionCache>, name: &'static str| {
            tokio::spawn(async move {
                cache
                    .answer(&cookie(name), async {
                        tokio::time::sleep(Duration::from_millis(300)).await;
                        signed_in(1)
                    })
                    .await
            })
        };
        let one = slow(cache.clone(), "x=1");
        let two = slow(cache.clone(), "x=2");
        tokio::time::sleep(Duration::from_millis(50)).await;
        let third = cache.answer(&cookie("x=3"), async { signed_in(3) }).await;
        assert!(matches!(third, CacheOutcome::Busy));
        assert!(matches!(one.await.unwrap(), CacheOutcome::SignedIn { .. }));
        assert!(matches!(two.await.unwrap(), CacheOutcome::SignedIn { .. }));
    }

    #[tokio::test]
    async fn errors_and_timeouts_are_signed_out() {
        let cache = SessionCache::new(Duration::from_secs(60), 16);
        for error in [
            CallError::Timeout,
            CallError::Unavailable("x".into()),
            CallError::Malformed("y".into()),
        ] {
            let outcome = cache
                .answer(&cookie("e=1"), async move { Err(error) })
                .await;
            assert!(matches!(outcome, CacheOutcome::SignedOut));
        }
    }

    #[tokio::test(start_paused = true)]
    async fn entries_expire_after_ttl() {
        let cache = SessionCache::new(Duration::from_secs(60), 16);
        let calls = AtomicUsize::new(0);
        let ask = || async {
            calls.fetch_add(1, Ordering::SeqCst);
            signed_in(5)
        };
        cache.answer(&cookie("t=1"), ask()).await;
        tokio::time::advance(Duration::from_secs(59)).await;
        cache.answer(&cookie("t=1"), ask()).await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        tokio::time::advance(Duration::from_secs(2)).await;
        cache.answer(&cookie("t=1"), ask()).await;
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn forget_users_drops_only_their_entries() {
        let cache = SessionCache::new(Duration::from_secs(60), 16);
        let calls = AtomicUsize::new(0);
        for (name, user) in [("u=5", 5), ("u=6", 6)] {
            cache.answer(&cookie(name), async { signed_in(user) }).await;
        }
        cache.forget_users(&[5]);
        for (name, user) in [("u=5", 5), ("u=6", 6)] {
            cache
                .answer(&cookie(name), async {
                    calls.fetch_add(1, Ordering::SeqCst);
                    signed_in(user)
                })
                .await;
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "only user 5 is asked again"
        );
    }
}
