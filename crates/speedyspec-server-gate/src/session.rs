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

/// Frees an ask's pending slot when its last caller goes away before it is answered (a closed tab, an aborted
/// request), so abandoned asks never fill `max_pending`.
struct PendingSlot<'a> {
    cache: &'a SessionCache,
    key: Key,
    cell: Arc<OnceCell<CacheOutcome>>,
}

impl Drop for PendingSlot<'_> {
    fn drop(&mut self) {
        if self.cell.initialized() {
            return;
        }
        let mut pending = self.cache.pending.lock().unwrap();
        // The map's reference and this one: nobody else is waiting on it.
        if pending
            .get(&self.key)
            .is_some_and(|cell| Arc::ptr_eq(cell, &self.cell))
            && Arc::strong_count(&self.cell) <= 2
        {
            pending.remove(&self.key);
        }
    }
}

/// See the module docs.
pub struct SessionCache {
    ttl: Duration,
    max_pending: usize,
    entries: Mutex<HashMap<Key, Entry>>,
    pending: Mutex<HashMap<Key, Arc<OnceCell<CacheOutcome>>>>,
    /// While set (the revocation feed is failing), every lookup asks and nothing is remembered.
    bypass: std::sync::atomic::AtomicBool,
    /// Bumped by every forget, under the `entries` lock. An ask remembers its answer only if no forget happened
    /// while it was in flight, so a sign-out or a revocation always wins over an answer asked for before it.
    forgets: std::sync::atomic::AtomicU64,
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
            bypass: std::sync::atomic::AtomicBool::new(false),
            forgets: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Stops (or starts again) remembering answers; while stopped, every lookup asks.
    pub fn set_bypass(&self, on: bool) {
        self.bypass.store(on, std::sync::atomic::Ordering::SeqCst);
    }

    fn bypassed(&self) -> bool {
        self.bypass.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// The remembered answer for `cookie`, or `ask`'s (shared with every concurrent lookup of the same cookie). A
    /// failed ask is signed out; only signed-in answers are remembered.
    pub async fn answer(
        &self,
        cookie: &HeaderValue,
        ask: impl Future<Output = Result<SessionAnswer, CallError>>,
    ) -> CacheOutcome {
        let key = key(cookie);
        if !self.bypassed()
            && let Some(entry) = self.entries.lock().unwrap().get(&key)
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
        let slot = PendingSlot {
            cache: self,
            key,
            cell,
        };
        let outcome = slot
            .cell
            .get_or_init(|| async {
                let asked_at = self.forgets.load(std::sync::atomic::Ordering::SeqCst);
                let outcome = match ask.await {
                    Ok(SessionAnswer::SignedIn { user, pages }) => {
                        let pages = Arc::new(pages);
                        let mut entries = self.entries.lock().unwrap();
                        if !self.bypassed()
                            && self.forgets.load(std::sync::atomic::Ordering::SeqCst) == asked_at
                        {
                            entries.insert(
                                key,
                                Entry {
                                    user,
                                    pages: pages.clone(),
                                    until: Instant::now() + self.ttl,
                                },
                            );
                        }
                        CacheOutcome::SignedIn { user, pages }
                    }
                    Ok(SessionAnswer::SignedOut) | Err(_) => CacheOutcome::SignedOut,
                };
                // Only this ask's own slot: a forget may have let a newer ask for the cookie take its place.
                let mut pending = self.pending.lock().unwrap();
                if pending
                    .get(&key)
                    .is_some_and(|cell| Arc::ptr_eq(cell, &slot.cell))
                {
                    pending.remove(&key);
                }
                outcome
            })
            .await
            .clone();
        outcome
    }

    /// Forgets the answer for this `Cookie` header (a sign-out), and any ask for it in flight: a later lookup asks
    /// again.
    pub fn forget_header(&self, cookie: &HeaderValue) {
        let key = key(cookie);
        self.forget(|entries| {
            entries.remove(&key);
        });
        self.pending.lock().unwrap().remove(&key);
    }

    /// Forgets every answer for these users (their access changed). An ask in flight is not yet known to be theirs,
    /// so every one is dropped: later lookups ask again.
    pub fn forget_users(&self, users: &[u64]) {
        self.forget(|entries| entries.retain(|_, entry| !users.contains(&entry.user)));
        self.pending.lock().unwrap().clear();
    }

    /// Forgets every answer, and every ask in flight.
    pub fn forget_all(&self) {
        self.forget(HashMap::clear);
        self.pending.lock().unwrap().clear();
    }

    /// Changes the entries and bumps `forgets` under one lock, so no ask in flight can remember past it.
    fn forget(&self, change: impl FnOnce(&mut HashMap<Key, Entry>)) {
        let mut entries = self.entries.lock().unwrap();
        self.forgets
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        change(&mut entries);
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

    /// Starts a lookup of `name` whose ask answers signed-in as user 5 only once `go` is notified, and waits until
    /// that ask has started.
    async fn ask_held_open(
        cache: &Arc<SessionCache>,
        name: &'static str,
    ) -> (
        tokio::task::JoinHandle<CacheOutcome>,
        Arc<tokio::sync::Notify>,
    ) {
        let (go, started) = (
            Arc::new(tokio::sync::Notify::new()),
            Arc::new(tokio::sync::Notify::new()),
        );
        let task = {
            let (cache, go, started) = (cache.clone(), go.clone(), started.clone());
            tokio::spawn(async move {
                cache
                    .answer(&cookie(name), async move {
                        started.notify_one();
                        go.notified().await;
                        signed_in(5)
                    })
                    .await
            })
        };
        tokio::time::timeout(Duration::from_secs(2), started.notified())
            .await
            .expect("the ask never started: the lookup shared an earlier one");
        (task, go)
    }

    /// Whether the next lookup of `name` asks again (nothing stale was remembered).
    async fn asks_again(cache: &SessionCache, name: &str) -> bool {
        let asked = AtomicUsize::new(0);
        cache
            .answer(&cookie(name), async {
                asked.fetch_add(1, Ordering::SeqCst);
                Ok(SessionAnswer::SignedOut)
            })
            .await;
        asked.load(Ordering::SeqCst) == 1
    }

    /// A sign-out, a changes entry or a reset while an ask is in flight wins over that ask's answer: it is never
    /// remembered.
    #[tokio::test]
    async fn a_forget_during_an_ask_keeps_its_answer_out() {
        let forgets: [(&str, fn(&SessionCache, &str)); 3] = [
            ("sign-out", |cache, name| cache.forget_header(&cookie(name))),
            ("changes", |cache, _| cache.forget_users(&[5])),
            ("reset", |cache, _| cache.forget_all()),
        ];
        for (what, forget) in forgets {
            let cache = Arc::new(SessionCache::new(Duration::from_secs(60), 16));
            let (task, go) = ask_held_open(&cache, "f=1").await;
            forget(&cache, "f=1");
            go.notify_one();
            assert!(matches!(task.await.unwrap(), CacheOutcome::SignedIn { .. }));
            assert!(
                asks_again(&cache, "f=1").await,
                "{what}: a stale answer was remembered"
            );
        }
    }

    /// A lookup that starts after a sign-out asks for itself instead of sharing the ask that was in flight before it.
    #[tokio::test]
    async fn a_lookup_after_a_sign_out_does_not_share_the_earlier_ask() {
        let cache = Arc::new(SessionCache::new(Duration::from_secs(60), 16));
        let (task, go) = ask_held_open(&cache, "g=1").await;
        cache.forget_header(&cookie("g=1"));
        let after = tokio::time::timeout(
            Duration::from_secs(2),
            cache.answer(&cookie("g=1"), async { Ok(SessionAnswer::SignedOut) }),
        )
        .await
        .expect("waited on the ask from before the sign-out");
        assert_eq!(after, CacheOutcome::SignedOut);
        go.notify_one();
        task.await.unwrap();
        assert!(
            asks_again(&cache, "g=1").await,
            "the earlier ask's answer was remembered"
        );
    }

    /// An ask that was in flight before a forget, finishing after a newer ask for the same cookie started, leaves
    /// the newer one in place: later lookups still share it.
    #[tokio::test]
    async fn an_older_ask_finishing_leaves_the_newer_one_shared() {
        let cache = Arc::new(SessionCache::new(Duration::from_secs(60), 16));
        let (old, old_go) = ask_held_open(&cache, "h=1").await;
        cache.forget_header(&cookie("h=1"));
        let (new, new_go) = ask_held_open(&cache, "h=1").await;
        old_go.notify_one();
        old.await.unwrap();
        let joined = {
            let cache = cache.clone();
            tokio::spawn(async move {
                cache
                    .answer(&cookie("h=1"), async {
                        panic!("should share the newer ask")
                    })
                    .await
            })
        };
        tokio::time::sleep(Duration::from_millis(20)).await;
        new_go.notify_one();
        let new = tokio::time::timeout(Duration::from_secs(2), new);
        let joined = tokio::time::timeout(Duration::from_secs(2), joined);
        assert!(matches!(
            new.await.unwrap().unwrap(),
            CacheOutcome::SignedIn { user: 5, .. }
        ));
        assert!(matches!(
            joined.await.expect("hung").unwrap(),
            CacheOutcome::SignedIn { user: 5, .. }
        ));
    }

    /// A lookup whose caller goes away (a closed tab, an aborted request) frees its slot: abandoned asks never fill
    /// `max_pending` for everyone else.
    #[tokio::test]
    async fn abandoned_asks_free_their_slot() {
        let cache = Arc::new(SessionCache::new(Duration::from_secs(60), 2));
        for name in ["a=1", "a=2", "a=3"] {
            let value = cookie(name);
            let slow = cache.answer(&value, async {
                tokio::time::sleep(Duration::from_secs(5)).await;
                signed_in(1)
            });
            assert!(
                tokio::time::timeout(Duration::from_millis(20), slow)
                    .await
                    .is_err(),
                "{name}: still asking, not busy"
            );
        }
        let fresh = cache.answer(&cookie("b=1"), async { signed_in(2) }).await;
        assert!(
            matches!(fresh, CacheOutcome::SignedIn { user: 2, .. }),
            "{fresh:?}"
        );
    }
}
