mod support;

use speedyspec_wp_cron::SpawnOutcome;
use speedyspec_wp_cron_contracts::DoingCron;
use support::*;

const LOCK: i64 = 60;

fn due(f: &Fixture) {
    f.store.set_raw(Some(option_with(&[(NOW - 10, "a")])));
}

#[test]
fn doing_cron_request_refuses() {
    let f = fixture();
    due(&f);
    assert_eq!(f.api.spawn_cron(0.0, true, LOCK).unwrap(), SpawnOutcome::No);
}

#[test]
fn fresh_lock_refuses() {
    let f = fixture();
    due(&f);
    f.doing.set(&format!("{:.22}", NOW as f64 - 30.0)).unwrap();
    assert_eq!(
        f.api.spawn_cron(0.0, false, LOCK).unwrap(),
        SpawnOutcome::No
    );
}

#[test]
fn lock_far_in_future_is_reset() {
    let f = fixture();
    due(&f);
    // More than ten minutes ahead: treated as no lock at all.
    f.doing.set(&format!("{:.22}", NOW as f64 + 700.0)).unwrap();
    assert!(matches!(
        f.api.spawn_cron(0.0, false, LOCK).unwrap(),
        SpawnOutcome::Spawn { .. }
    ));
}

#[test]
fn no_ready_jobs_refuses() {
    let f = fixture();
    f.store.set_raw(Some(option_with(&[(NOW + 100, "later")])));
    assert_eq!(
        f.api.spawn_cron(0.0, false, LOCK).unwrap(),
        SpawnOutcome::No
    );
}

#[test]
fn first_job_in_future_refuses() {
    let f = fixture();
    // A plugin's ready list whose first entry is still ahead.
    f.answer(
        "pre_get_ready_cron_jobs",
        option_with(&[(NOW + 100, "later")]),
    );
    assert_eq!(
        f.api.spawn_cron(0.0, false, LOCK).unwrap(),
        SpawnOutcome::No
    );
}

#[test]
fn due_job_spawns_sets_lock_and_wakes_now() {
    let f = fixture();
    due(&f);
    let stale = format!("{:.22}", NOW as f64 - 120.0);
    f.doing.set(&stale).unwrap();
    let expected = "1700000000.5000000000000000000000".to_string();
    assert_eq!(
        f.api.spawn_cron(0.0, false, LOCK).unwrap(),
        SpawnOutcome::Spawn {
            doing_wp_cron: expected.clone()
        }
    );
    assert_eq!(f.doing.get().unwrap(), Some(expected));
    assert!(f.wake.earliest().is_some_and(|t| t <= NOW));
    // The given time, when there is one.
    let g = fixture();
    due(&g);
    assert_eq!(
        g.api.spawn_cron(NOW as f64 + 5.25, false, LOCK).unwrap(),
        SpawnOutcome::Spawn {
            doing_wp_cron: "1700000005.2500000000000000000000".to_string()
        }
    );
}
