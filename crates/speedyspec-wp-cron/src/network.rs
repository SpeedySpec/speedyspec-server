//! The network walk: on multisite, one call runs every active site's due events (Iterator over the sites, resuming
//! after the site where the last call stopped). WordPress runs a subsite's cron only when the subsite is visited.

use std::time::Duration;

use speedyspec_wp_cron_contracts::{CronError, RunSummary};

use crate::CronApi;

impl CronApi {
    /// Runs the due events of every active site of the network within `budget`, starting after the site where the
    /// last walk stopped; on a single site, [`CronApi::run_due_events`]. Each site is switched to, run and restored
    /// (restored also when its run fails), and the cursor moves to it, so a failing or budget-ending site is the
    /// last of this call and the next call starts after it.
    pub fn run_network_due_events(&self, budget: Duration) -> Result<RunSummary, CronError> {
        if !self.p.network.is_multisite() {
            return self.run_due_events(budget);
        }
        let mut sites = self.p.network.site_ids()?;
        sites.sort_unstable();
        sites.dedup();
        let mut total = RunSummary {
            ran: 0,
            claimed_by_others: 0,
            unclaimed: 0,
            next: None,
            sites: 0,
        };
        if sites.is_empty() {
            return Ok(total);
        }
        let start = self
            .p
            .network
            .cursor()?
            .and_then(|cursor| sites.iter().position(|&site| site > cursor))
            .unwrap_or(0);
        let started = self.p.clock.microtime();
        let mut stopped = false;
        for k in 0..sites.len() {
            let site = sites[(start + k) % sites.len()];
            let elapsed = (self.p.clock.microtime() - started).max(0.0);
            let remaining = budget.saturating_sub(Duration::from_secs_f64(elapsed));
            self.p.switcher.switch_to(site)?;
            let run = self.run_due_events(remaining);
            let restored = self.p.switcher.restore();
            self.p.network.set_cursor(site)?;
            let summary = run?;
            restored?;
            total.ran += summary.ran;
            total.claimed_by_others += summary.claimed_by_others;
            total.unclaimed += summary.unclaimed;
            total.sites += 1;
            if let Some(next) = summary.next {
                total.next = Some(total.next.map_or(next, |t| t.min(next)));
            }
            if self.p.clock.microtime() - started >= budget.as_secs_f64() {
                stopped = true;
                break;
            }
        }
        if stopped {
            total.next = Some(self.p.clock.time());
        }
        if let Some(next) = total.next {
            self.p.wake.wake_at(next);
        }
        Ok(total)
    }
}
