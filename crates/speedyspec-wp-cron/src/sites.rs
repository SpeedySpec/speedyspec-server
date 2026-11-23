//! The subsites to call: on a network, which other sites have an event due now. Each is then run by its own request,
//! booted as that site with its own plugins and theme; `switch_to_blog()` loads neither, so nothing runs here.

use speedyspec_wp_cron_contracts::{CronError, SiteRef};

use crate::CronApi;

impl CronApi {
    /// The network's other active sites with an event due now, in id order; `[]` on a single site. Each site's
    /// `cron` option is read while switched to it, and nothing is run. The earliest future due time across the sites
    /// read is reported to the wake slot, so the server wakes for a site that is not due yet.
    pub fn due_sites(&self) -> Result<Vec<SiteRef>, CronError> {
        if !self.p.network.is_multisite() {
            return Ok(Vec::new());
        }
        let current = self.p.network.current();
        let now = self.p.clock.time();
        let mut due = Vec::new();
        let mut wake: Option<i64> = None;
        for site in self.p.network.sites()? {
            if site.id == current {
                continue;
            }
            let switched = self.p.switcher.switch_to(site.id);
            // WordPress pushes its stack before `switch_blog` fires, so a failed switch is restored too.
            let read = switched.and_then(|()| self.get_cron_array());
            let restored = self.p.switcher.restore();
            let cron = read?;
            restored?;
            let mut is_due = false;
            for event in cron.events() {
                if event.timestamp <= now {
                    is_due = true;
                } else {
                    wake = Some(wake.map_or(event.timestamp, |w| w.min(event.timestamp)));
                }
            }
            if is_due {
                due.push(site);
            }
        }
        if let Some(next) = wake {
            self.p.wake.wake_at(next);
        }
        Ok(due)
    }
}
