//! `spawn_cron()`: WordPress's checks, with a request for an immediate scheduled call in place of the loopback.

use speedyspec_wp_cron_contracts::CronError;

use crate::CronApi;
use crate::php::{array_cast, key_after, numeric_prefix};

/// What `spawn_cron()` decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpawnOutcome {
    /// `false`: a check refused.
    No,
    /// A run is due: the `doing_cron` value set, which the PHP side puts in `cron_request`'s URL and key.
    Spawn { doing_wp_cron: String },
}

impl CronApi {
    /// `spawn_cron( $gmt_time )`'s checks, in WordPress's order: a cron request already running (`DOING_CRON` or
    /// `$_GET['doing_wp_cron']`, which the caller passes as `doing_cron_request`), the `doing_cron` lock within
    /// `lock_timeout` (`WP_CRON_LOCK_TIMEOUT`, a lock more than ten minutes ahead counting as none), no ready jobs, the
    /// first ready job still ahead. When they pass, `doing_cron` is set as WordPress sets it and the wake time
    /// reported is now, so the server makes the scheduled call at once instead of a loopback request.
    pub fn spawn_cron(
        &self,
        gmt_time: f64,
        doing_cron_request: bool,
        lock_timeout: i64,
    ) -> Result<SpawnOutcome, CronError> {
        let gmt_time = if gmt_time == 0.0 {
            self.p.clock.microtime()
        } else {
            gmt_time
        };
        if doing_cron_request {
            return Ok(SpawnOutcome::No);
        }
        let mut lock = self.p.doing_cron.get()?.map_or(0.0, |v| numeric_prefix(&v));
        if lock > gmt_time + 600.0 {
            lock = 0.0;
        }
        if lock + lock_timeout as f64 > gmt_time {
            return Ok(SpawnOutcome::No);
        }
        let crons = array_cast(&self.get_ready_cron_jobs()?.read());
        if crons.is_empty() {
            return Ok(SpawnOutcome::No);
        }
        if key_after(&crons[0].0, gmt_time) {
            return Ok(SpawnOutcome::No);
        }
        let doing_wp_cron = format!("{gmt_time:.22}");
        self.p.doing_cron.set(&doing_wp_cron)?;
        self.p.wake.wake_at(gmt_time as i64);
        Ok(SpawnOutcome::Spawn { doing_wp_cron })
    }
}
