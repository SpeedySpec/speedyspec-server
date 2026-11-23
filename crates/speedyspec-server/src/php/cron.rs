//! The cron scheduler: calls each PHP app's declared cron path when the app last said it is next due, and at least
//! every `max_interval`.
//!
//! [`CronScheduler`] is an Active Object: one task per app owns the wake time, and every input reaches it as a message
//! (the `next_cron` of each worker report, through the [`WakeSink`] it hands out) or as the end of its own call. It
//! never reads the app's data and knows nothing of WordPress: a path, a time, and a number in a report. Every replica
//! runs its own; the app makes sure a due event runs once (its claims), so replicas never coordinate.
//!
//! A call's answer may list follow-up calls (on a WordPress network, one per subsite with a due event, each booted as
//! that site): opaque query strings the scheduler appends to the cron path, called in order right after.

use std::sync::Arc;
use std::time::Duration;

use speedyspec_server_domain::AppId;
use speedyspec_server_gate::{BoxFuture, CallError, MaintenanceSet};
use tokio::time::Instant;

use crate::config::CronConfig;
use crate::php::WakeSink;

/// The scheduler's call to an app's cron path.
pub trait CronCalls: Send + Sync {
    /// `GET <path>?budget=<seconds>` (then `&<query>` for a follow-up) on the app's cron lane, bounded by `timeout`.
    /// A 2xx answer is `Ok` with the follow-up queries it lists.
    fn call<'a>(
        &'a self,
        app: &'a AppId,
        path: &'a str,
        query: Option<&'a str>,
        budget: Duration,
        timeout: Duration,
    ) -> BoxFuture<'a, Result<Vec<String>, CallError>>;
}

/// Awaits `call`, folding every report that arrives meanwhile into `reported` (the earliest wins).
async fn collecting_reports(
    call: BoxFuture<'_, Result<Vec<String>, CallError>>,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<u64>,
    reported: &mut Option<Instant>,
) -> Result<Vec<String>, CallError> {
    tokio::pin!(call);
    loop {
        // Reports first: the call's own report is queued before the call ends.
        tokio::select! {
            biased;
            Some(next) = rx.recv() => {
                let at = instant_for(next);
                *reported = Some(reported.map_or(at, |r| r.min(at)));
            }
            result = &mut call => return result,
        }
    }
}

/// The app's cron path and settings, read every round (so a switch or a new release applies at once); `None` when
/// the live release declares no path or the operator disabled cron.
pub type CronTarget = Arc<dyn Fn() -> Option<(String, CronConfig)> + Send + Sync>;

/// The shortest time between two calls, so an app that keeps reporting "due now" is not called in a busy loop.
const MIN_GAP: Duration = Duration::from_secs(1);
/// How long to wait before looking again while the app has no cron path.
const IDLE: Duration = Duration::from_secs(60);
/// The first wait after a failed call; it doubles up to `max_interval`.
const FIRST_BACKOFF: Duration = Duration::from_secs(5);

/// When a report's Unix time falls, on the tokio clock; a time not ahead is now.
fn instant_for(unix: u64) -> Instant {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    Instant::now() + Duration::from_secs(unix.saturating_sub(now))
}

/// Calls an app's cron path when it is due.
pub struct CronScheduler;

impl CronScheduler {
    /// Starts `app`'s scheduler on the current tokio runtime. The returned sink takes each `next_cron` the app's
    /// workers report: a time earlier than the planned wake moves it forward, never back.
    ///
    /// While `paused` is set (the generation is draining for a server switch) it makes no call, so the old and the new
    /// generation of one replica never both call.
    pub fn spawn(
        app: AppId,
        target: CronTarget,
        calls: Arc<dyn CronCalls>,
        maintenance: Arc<MaintenanceSet>,
        paused: Arc<std::sync::atomic::AtomicBool>,
    ) -> (tokio::task::JoinHandle<()>, WakeSink) {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<u64>();
        let sink: WakeSink = Arc::new(move |next| {
            let _ = tx.send(next);
        });
        let task = tokio::spawn(async move {
            let mut next_wake = Instant::now();
            // No call before this, whatever is reported: the floor between two calls.
            let mut not_before = Instant::now();
            let mut backoff: Option<Duration> = None;
            // The follow-up the last round ended on: the next round starts after it, so no site always comes last.
            let mut last_follow_up: Option<String> = None;
            loop {
                tokio::select! {
                    _ = tokio::time::sleep_until(next_wake.max(not_before)) => {}
                    Some(reported) = rx.recv() => {
                        next_wake = next_wake.min(instant_for(reported));
                        continue;
                    }
                }
                if paused.load(std::sync::atomic::Ordering::SeqCst) {
                    next_wake =
                        Instant::now() + IDLE.min(target().map_or(IDLE, |(_, c)| c.max_interval));
                    continue;
                }
                let Some((path, config)) = target() else {
                    next_wake = Instant::now() + IDLE;
                    continue;
                };
                if maintenance.is_on(&app) {
                    next_wake = Instant::now() + config.max_interval;
                    continue;
                }
                let round_started = Instant::now();
                let mut started = round_started;
                let mut reported: Option<Instant> = None;
                let result = collecting_reports(
                    calls.call(&app, &path, None, config.budget, config.timeout),
                    &mut rx,
                    &mut reported,
                )
                .await;
                // The follow-ups, in order, starting after the one the last round ended on, until `max_interval` from
                // the round's start (so the app's own call is never held back longer); their own lists are not
                // followed (one level). Sites not reached are still due, so the next round lists them again.
                let list = result.as_deref().unwrap_or_default();
                let list_was_empty = list.is_empty();
                let start = last_follow_up
                    .as_ref()
                    .and_then(|last| list.iter().position(|q| q == last))
                    .map_or(0, |i| i + 1);
                for query in list[start.min(list.len())..]
                    .iter()
                    .chain(&list[..start.min(list.len())])
                {
                    if Instant::now() >= round_started + config.max_interval
                        || paused.load(std::sync::atomic::Ordering::SeqCst)
                        || maintenance.is_on(&app)
                    {
                        break;
                    }
                    // A switch mid-list: the live release's path, or no more calls when it declares none.
                    let Some((path, _)) = target() else {
                        break;
                    };
                    started = Instant::now();
                    let follow_up = collecting_reports(
                        calls.call(&app, &path, Some(query), config.budget, config.timeout),
                        &mut rx,
                        &mut reported,
                    )
                    .await;
                    if let Err(err) = follow_up {
                        tracing::warn!(event = "php_cron_follow_up_failed", app = %app, query = %query, error = %err);
                    }
                    last_follow_up = Some(query.clone());
                }
                let now = Instant::now();
                next_wake = match result {
                    Ok(_) => {
                        backoff = None;
                        reported.unwrap_or(now + config.max_interval)
                    }
                    Err(err) => {
                        tracing::warn!(event = "php_cron_call_failed", app = %app, error = %err);
                        let wait = backoff
                            .map_or(FIRST_BACKOFF, |b| b * 2)
                            .min(config.max_interval);
                        backoff = Some(wait);
                        now + wait
                    }
                };
                next_wake = next_wake.min(now + config.max_interval);
                if !list_was_empty {
                    // After follow-ups, the next round is due `max_interval` from this one's start, not its end.
                    next_wake = next_wake.min(round_started + config.max_interval);
                }
                not_before = started + MIN_GAP;
            }
        });
        (task, sink)
    }
}

/// Starts a scheduler for every php app, each reporting through [`PhpRuntime::set_wake_sink`]. A scheduler whose
/// app's live release declares no `php.cron` path, or whose `cron.enabled` is false, makes no call.
pub fn spawn_all(
    config: &crate::config::Config,
    php: Arc<crate::php::PhpRuntime>,
    maintenance: Arc<MaintenanceSet>,
    draining: Arc<std::sync::atomic::AtomicBool>,
) -> Vec<tokio::task::JoinHandle<()>> {
    let calls: Arc<dyn CronCalls> = Arc::new(crate::php::calls::FastCgiGateCalls::new(
        php.clone(),
        &config.mounts,
        config.health.timeout,
    ));
    config
        .apps
        .iter()
        .filter_map(|(app, settings)| settings.php.as_ref().map(|p| (app.clone(), p.cron)))
        .map(|(app, cron)| {
            let (runtime, id) = (php.clone(), app.clone());
            let target: CronTarget = Arc::new(move || {
                if !cron.enabled {
                    return None;
                }
                let path = runtime.live_manifest(&id)?.php_section()?.cron?.path;
                Some((path, cron))
            });
            let (task, sink) = CronScheduler::spawn(
                app.clone(),
                target,
                calls.clone(),
                maintenance.clone(),
                draining.clone(),
            );
            php.set_wake_sink(&app, sink);
            task
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::time::Duration;

    use speedyspec_server_domain::AppId;
    use speedyspec_server_gate::{BoxFuture, CallError, MaintenanceSet, MaintenanceSource};
    use tokio::time::Instant;

    use crate::config::CronConfig;

    /// Records when each call started; answers from a script (`Ok` once the list runs out), each taking `takes`.
    struct Fake {
        calls: Mutex<Vec<(Instant, String)>>,
        answers: Mutex<Vec<Result<Vec<String>, CallError>>>,
        takes: Duration,
        /// Reported to the sink during the call, as a worker report would be: seconds from now.
        report: Mutex<Option<(WakeSinkSlot, i64)>>,
    }

    type WakeSinkSlot = std::sync::Arc<std::sync::OnceLock<crate::php::WakeSink>>;

    impl CronCalls for Fake {
        fn call<'a>(
            &'a self,
            _app: &'a AppId,
            path: &'a str,
            query: Option<&'a str>,
            _budget: Duration,
            _timeout: Duration,
        ) -> BoxFuture<'a, Result<Vec<String>, CallError>> {
            Box::pin(async move {
                let called = match query {
                    Some(query) => format!("{path}&{query}"),
                    None => path.to_string(),
                };
                self.calls.lock().unwrap().push((Instant::now(), called));
                tokio::time::sleep(self.takes).await;
                let report = self.report.lock().unwrap().clone();
                if let Some((slot, seconds)) = report
                    && let Some(sink) = slot.get()
                {
                    sink((unix_now() + seconds).max(0) as u64);
                }
                let mut answers = self.answers.lock().unwrap();
                if answers.is_empty() {
                    Ok(Vec::new())
                } else {
                    answers.remove(0)
                }
            })
        }
    }

    fn unix_now() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
    }

    fn config(max_interval: u64) -> CronConfig {
        CronConfig {
            enabled: true,
            max_interval: Duration::from_secs(max_interval),
            timeout: Duration::from_secs(120),
            budget: Duration::from_secs(30),
        }
    }

    fn fake() -> std::sync::Arc<Fake> {
        std::sync::Arc::new(Fake {
            calls: Mutex::new(Vec::new()),
            answers: Mutex::new(Vec::new()),
            takes: Duration::from_millis(10),
            report: Mutex::new(None),
        })
    }

    fn app() -> AppId {
        AppId::parse("admin-api").unwrap()
    }

    struct Running {
        fake: std::sync::Arc<Fake>,
        sink: crate::php::WakeSink,
        task: tokio::task::JoinHandle<()>,
        started: Instant,
        maintenance: std::sync::Arc<MaintenanceSet>,
        paused: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    impl Drop for Running {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    impl Running {
        /// When each call started, in seconds from the start (rounded down).
        fn call_seconds(&self) -> Vec<u64> {
            self.fake
                .calls
                .lock()
                .unwrap()
                .iter()
                .map(|(at, _)| (*at - self.started).as_secs())
                .collect()
        }
    }

    fn start(fake: std::sync::Arc<Fake>, target: CronTarget) -> Running {
        let maintenance = std::sync::Arc::new(MaintenanceSet::default());
        let started = Instant::now();
        let paused = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (task, sink) = CronScheduler::spawn(
            app(),
            target,
            fake.clone(),
            maintenance.clone(),
            paused.clone(),
        );
        Running {
            fake,
            sink,
            task,
            started,
            maintenance,
            paused,
        }
    }

    fn fixed(max_interval: u64) -> CronTarget {
        std::sync::Arc::new(move || Some(("/cron.php".to_string(), config(max_interval))))
    }

    #[tokio::test(start_paused = true)]
    async fn calls_at_start_then_every_max_interval() {
        let r = start(fake(), fixed(60));
        tokio::time::sleep(Duration::from_secs(185)).await;
        assert_eq!(r.call_seconds(), vec![0, 60, 120, 180]);
    }

    #[tokio::test(start_paused = true)]
    async fn report_pulls_wake_forward() {
        let r = start(fake(), fixed(60));
        tokio::time::sleep(Duration::from_secs(5)).await;
        (r.sink)((unix_now() + 10) as u64);
        tokio::time::sleep(Duration::from_secs(20)).await;
        assert_eq!(r.call_seconds(), vec![0, 15]);
    }

    #[tokio::test(start_paused = true)]
    async fn later_report_does_not_push_wake_back() {
        let r = start(fake(), fixed(60));
        tokio::time::sleep(Duration::from_secs(5)).await;
        (r.sink)((unix_now() + 10) as u64);
        (r.sink)((unix_now() + 50) as u64);
        tokio::time::sleep(Duration::from_secs(20)).await;
        assert_eq!(r.call_seconds(), vec![0, 15]);
    }

    #[tokio::test(start_paused = true)]
    async fn past_wake_calls_once_at_a_time() {
        let mut slow = std::sync::Arc::into_inner(fake()).unwrap();
        slow.takes = Duration::from_secs(3);
        let slow = std::sync::Arc::new(slow);
        let slot: WakeSinkSlot = std::sync::Arc::new(std::sync::OnceLock::new());
        // Every call reports "due now" (a time in the past).
        *slow.report.lock().unwrap() = Some((slot.clone(), -100));
        let r = start(slow, fixed(60));
        let _ = slot.set(r.sink.clone());
        tokio::time::sleep(Duration::from_secs(10)).await;
        let calls = r.call_seconds();
        // One call at a time, each after the previous ended, never closer than the floor between calls.
        for pair in calls.windows(2) {
            assert!(pair[1] >= pair[0] + 3, "{calls:?}");
        }
        assert!(calls.len() >= 2, "{calls:?}");
    }

    #[tokio::test(start_paused = true)]
    async fn report_during_call_sets_next_wake() {
        let reporting = fake();
        let slot: WakeSinkSlot = std::sync::Arc::new(std::sync::OnceLock::new());
        *reporting.report.lock().unwrap() = Some((slot.clone(), 20));
        let r = start(reporting, fixed(60));
        let _ = slot.set(r.sink.clone());
        tokio::time::sleep(Duration::from_secs(45)).await;
        assert_eq!(r.call_seconds(), vec![0, 20, 40]);
    }

    #[tokio::test(start_paused = true)]
    async fn failure_backs_off_5_10_20_capped() {
        let failing = fake();
        *failing.answers.lock().unwrap() = vec![Err(CallError::Timeout); 6];
        let r = start(failing, fixed(30));
        tokio::time::sleep(Duration::from_secs(100)).await;
        // 0, +5, +10, +20, then capped at max_interval (30): 0, 5, 15, 35, 65, 95.
        assert_eq!(r.call_seconds(), vec![0, 5, 15, 35, 65, 95]);
    }

    #[tokio::test(start_paused = true)]
    async fn no_call_in_maintenance() {
        let fake = fake();
        let maintenance = std::sync::Arc::new(MaintenanceSet::default());
        maintenance.set(&app(), MaintenanceSource::Operator, true);
        let (task, _sink) = CronScheduler::spawn(
            app(),
            fixed(60),
            fake.clone(),
            maintenance.clone(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        tokio::time::sleep(Duration::from_secs(130)).await;
        assert!(fake.calls.lock().unwrap().is_empty());
        maintenance.set(&app(), MaintenanceSource::Operator, false);
        tokio::time::sleep(Duration::from_secs(61)).await;
        assert_eq!(fake.calls.lock().unwrap().len(), 1);
        task.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn disabled_cron_makes_no_calls() {
        let r = start(fake(), std::sync::Arc::new(|| None));
        tokio::time::sleep(Duration::from_secs(300)).await;
        assert!(r.call_seconds().is_empty());
        let _ = &r.maintenance;
    }

    #[tokio::test(start_paused = true)]
    async fn switch_keeps_one_scheduler() {
        let path = std::sync::Arc::new(Mutex::new("/cron.php".to_string()));
        let current = path.clone();
        let target: CronTarget =
            std::sync::Arc::new(move || Some((current.lock().unwrap().clone(), config(60))));
        let r = start(fake(), target);
        tokio::time::sleep(Duration::from_secs(30)).await;
        *path.lock().unwrap() = "/new-cron.php".to_string();
        tokio::time::sleep(Duration::from_secs(60)).await;
        let paths: Vec<String> = r
            .fake
            .calls
            .lock()
            .unwrap()
            .iter()
            .map(|(_, p)| p.clone())
            .collect();
        assert_eq!(
            paths,
            vec!["/cron.php".to_string(), "/new-cron.php".to_string()]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn fast_calls_reporting_now_stay_a_second_apart() {
        let quick = fake();
        let slot: WakeSinkSlot = std::sync::Arc::new(std::sync::OnceLock::new());
        // Each 10 ms call reports "due now", as an app with events it cannot claim would.
        *quick.report.lock().unwrap() = Some((slot.clone(), -100));
        let r = start(quick, fixed(60));
        let _ = slot.set(r.sink.clone());
        tokio::time::sleep(Duration::from_millis(5500)).await;
        let calls: Vec<Instant> = r
            .fake
            .calls
            .lock()
            .unwrap()
            .iter()
            .map(|(at, _)| *at)
            .collect();
        for pair in calls.windows(2) {
            assert!(
                pair[1] - pair[0] >= Duration::from_secs(1),
                "{:?}",
                r.call_seconds()
            );
        }
        // A visitor's "now" does not skip the floor either.
        let before = calls.len();
        (r.sink)(0);
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(r.fake.calls.lock().unwrap().len() <= before + 1);
    }

    impl Running {
        /// What each call asked for: the path, then `&` and the follow-up's query.
        fn called(&self) -> Vec<String> {
            self.fake
                .calls
                .lock()
                .unwrap()
                .iter()
                .map(|(_, c)| c.clone())
                .collect()
        }
    }

    fn follow_ups(queries: &[&str]) -> Result<Vec<String>, CallError> {
        Ok(queries.iter().map(|q| q.to_string()).collect())
    }

    #[tokio::test(start_paused = true)]
    async fn follow_ups_called_in_order() {
        let f = fake();
        *f.answers.lock().unwrap() = vec![follow_ups(&["site=2", "site=3"])];
        let r = start(f, fixed(60));
        tokio::time::sleep(Duration::from_secs(5)).await;
        assert_eq!(
            r.called(),
            vec!["/cron.php", "/cron.php&site=2", "/cron.php&site=3"]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn follow_ups_are_one_level() {
        let f = fake();
        *f.answers.lock().unwrap() = vec![follow_ups(&["site=2"]), follow_ups(&["site=9"])];
        let r = start(f, fixed(60));
        tokio::time::sleep(Duration::from_secs(5)).await;
        assert_eq!(r.called(), vec!["/cron.php", "/cron.php&site=2"]);
    }

    #[tokio::test(start_paused = true)]
    async fn failed_follow_up_continues() {
        let f = fake();
        *f.answers.lock().unwrap() =
            vec![follow_ups(&["site=2", "site=3"]), Err(CallError::Timeout)];
        let r = start(f, fixed(60));
        tokio::time::sleep(Duration::from_secs(65)).await;
        assert_eq!(
            r.called(),
            vec![
                "/cron.php",
                "/cron.php&site=2",
                "/cron.php&site=3",
                "/cron.php"
            ],
            "no backoff either: the main call succeeded"
        );
        assert_eq!(r.call_seconds()[3], 60);
    }

    #[tokio::test(start_paused = true)]
    async fn maintenance_stops_the_list() {
        let slow = taking(2);
        *slow.answers.lock().unwrap() = vec![follow_ups(&["site=2", "site=3", "site=4"])];
        let r = start(slow, fixed(60));
        // During the first follow-up (2 s to 4 s), the app goes into maintenance.
        tokio::time::sleep(Duration::from_secs(3)).await;
        r.maintenance.set(&app(), MaintenanceSource::Operator, true);
        tokio::time::sleep(Duration::from_secs(10)).await;
        assert_eq!(r.called(), vec!["/cron.php", "/cron.php&site=2"]);
    }

    fn taking(seconds: u64) -> std::sync::Arc<Fake> {
        let mut slow = std::sync::Arc::into_inner(fake()).unwrap();
        slow.takes = Duration::from_secs(seconds);
        std::sync::Arc::new(slow)
    }

    #[tokio::test(start_paused = true)]
    async fn long_list_keeps_max_interval_and_rotates() {
        let slow = taking(4);
        *slow.answers.lock().unwrap() = vec![
            follow_ups(&["a", "b", "c"]),
            Ok(Vec::new()),
            Ok(Vec::new()),
            follow_ups(&["a", "b", "c"]),
        ];
        let r = start(slow, fixed(10));
        tokio::time::sleep(Duration::from_secs(23)).await;
        // Round 1 (deadline 10): main 0-4, a 4-8, b 8-12. Round 2 at 12 (deadline 22) starts after b: c, then a.
        assert_eq!(
            r.called(),
            vec![
                "/cron.php",
                "/cron.php&a",
                "/cron.php&b",
                "/cron.php",
                "/cron.php&c",
                "/cron.php&a"
            ]
        );
        assert_eq!(
            r.call_seconds()[3],
            12,
            "the app's own call is not held back"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn draining_stops_the_list() {
        let slow = taking(2);
        *slow.answers.lock().unwrap() = vec![follow_ups(&["site=2", "site=3"])];
        let r = start(slow, fixed(60));
        tokio::time::sleep(Duration::from_secs(3)).await;
        r.paused.store(true, std::sync::atomic::Ordering::SeqCst);
        tokio::time::sleep(Duration::from_secs(10)).await;
        assert_eq!(r.called(), vec!["/cron.php", "/cron.php&site=2"]);
    }

    #[tokio::test(start_paused = true)]
    async fn switch_mid_list_uses_the_live_path() {
        let path = std::sync::Arc::new(Mutex::new(Some("/cron.php".to_string())));
        let current = path.clone();
        let target: CronTarget =
            std::sync::Arc::new(move || current.lock().unwrap().clone().map(|p| (p, config(60))));
        let slow = taking(2);
        *slow.answers.lock().unwrap() = vec![follow_ups(&["site=2", "site=3", "site=4"])];
        let r = start(slow, target);
        tokio::time::sleep(Duration::from_secs(3)).await;
        *path.lock().unwrap() = Some("/new.php".to_string());
        tokio::time::sleep(Duration::from_secs(2)).await;
        *path.lock().unwrap() = None;
        tokio::time::sleep(Duration::from_secs(10)).await;
        assert_eq!(
            r.called(),
            vec!["/cron.php", "/cron.php&site=2", "/new.php&site=3"]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn follow_up_report_pulls_wake() {
        let reporting = fake();
        let slot: WakeSinkSlot = std::sync::Arc::new(std::sync::OnceLock::new());
        *reporting.report.lock().unwrap() = Some((slot.clone(), 20));
        *reporting.answers.lock().unwrap() = vec![follow_ups(&["site=2"])];
        let r = start(reporting, fixed(60));
        let _ = slot.set(r.sink.clone());
        tokio::time::sleep(Duration::from_secs(25)).await;
        assert_eq!(r.call_seconds(), vec![0, 0, 20]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_draining_generation_makes_no_calls() {
        let fake = fake();
        let maintenance = std::sync::Arc::new(MaintenanceSet::default());
        let paused = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let (task, sink) =
            CronScheduler::spawn(app(), fixed(10), fake.clone(), maintenance, paused.clone());
        sink(0);
        tokio::time::sleep(Duration::from_secs(35)).await;
        assert!(
            fake.calls.lock().unwrap().is_empty(),
            "the old generation stays quiet while it drains"
        );
        // Resumed (the switch was abandoned): calls start again within max_interval.
        paused.store(false, std::sync::atomic::Ordering::SeqCst);
        tokio::time::sleep(Duration::from_secs(11)).await;
        assert_eq!(fake.calls.lock().unwrap().len(), 1);
        task.abort();
    }
}
