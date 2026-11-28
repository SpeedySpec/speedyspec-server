//! The post writers' cron port over the Rust cron scheduling core (Adapter): `wp_clear_scheduled_hook()`,
//! `wp_schedule_single_event()` and `wp_next_scheduled()` as [`CronApi`] runs them, without the PHP wrappers' trip
//! through PHP. The runtime binds it as the default of the posts area's `SpeedySpec\WpPosts\Scheduling` port; PHP may
//! still bind its own.
//!
//! It lives in the runtime, which wires the seams together: the posts seam's crates depend on the cron contracts
//! only, never on the cron implementation.

use std::rc::Rc;

use speedyspec_wp_cron::CronApi;
use speedyspec_wp_cron_contracts::{CronError, CronResult};
use speedyspec_wp_hooks_contracts::HookValue;
use speedyspec_wp_posts::Scheduling;

/// [`Scheduling`] over the worker's [`CronApi`].
pub struct CronScheduling(Rc<CronApi>);

impl CronScheduling {
    pub fn new(cron: Rc<CronApi>) -> CronScheduling {
        CronScheduling(cron)
    }
}

fn hook(name: &str) -> HookValue {
    HookValue::Str(name.to_owned())
}

impl Scheduling for CronScheduling {
    /// `wp_clear_scheduled_hook( $hook, $args )`, `$wp_error` false.
    fn clear_scheduled_hook(&self, name: &str, args: &HookValue) -> Result<CronResult, CronError> {
        self.0.clear_scheduled_hook(&hook(name), args, false)
    }

    /// `wp_schedule_single_event( $timestamp, $hook, $args )`, `$wp_error` false.
    fn schedule_single_event(
        &self,
        timestamp: &HookValue,
        name: &str,
        args: &HookValue,
    ) -> Result<CronResult, CronError> {
        self.0
            .schedule_single_event(timestamp, &hook(name), args, false)
    }

    /// `wp_next_scheduled( $hook, $args )`.
    fn next_scheduled(&self, name: &str, args: &HookValue) -> Result<HookValue, CronError> {
        self.0.next_scheduled(&hook(name), args)
    }
}
