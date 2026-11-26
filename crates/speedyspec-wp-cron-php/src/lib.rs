//! WordPress's cron functions (`wp-includes/cron.php`) as native PHP functions over [`speedyspec_wp_cron::CronApi`].
//!
//! - [`CronArea`] declares the natives (`SpeedySpec\WpCron\Native\…`) and evaluates `cron.php`, the thin wrapper
//!   that declares WordPress's global functions over them, builds `WP_Error`s from the natives' markers, and keeps
//!   `wp_clear_scheduled_hook()`'s pre-3.0 discrete arguments (which need `func_get_args()`).
//! - The `cron` option is read and written through the options port ([`speedyspec_wp_cron_options::OptionCronStore`]),
//!   every write a compare-and-swap.
//! - [`prepare_release`] writes a copy of WordPress whose `cron.php` lacks the ported functions.
//!
//! Deliberate differences from WordPress are listed in the cron port's design: concurrent writes retry instead of
//! overwriting each other, and a write still conflicting after five attempts returns `could_not_set` (or `false`).

mod adapters;
mod natives;
mod release;

use std::rc::Rc;

use speedyspec_php::NativeArea;
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_wp_cron::{CronApi, CronParts};
use speedyspec_wp_cron_contracts::WakeReporter;
use speedyspec_wp_cron_options::OptionCronStore;
use speedyspec_wp_hooks::WpHookApi;
use speedyspec_wp_options::OptionsApi;

pub use adapters::{
    PhpArgsKeyer, PhpDoingCron, PhpErrorReporter, PhpEventObjects, PhpNetworkSites,
    PhpSiteSwitcher, PhpTranslator, SystemClock,
};
pub use release::{CRON_ENDPOINT, PORTED_FUNCTIONS, prepare_release, strip_ported};

const FACADE: &str = include_str!("cron.php");

/// The compatibility runner for the `cron` scenarios: `compat/cron/setup.php` on a copy of `release` prepared for the
/// native functions.
pub fn cron_scenario_runner(
    php: speedyspec_php::Php,
    release: &std::path::Path,
) -> Result<speedyspec_compat::PhpRunner, String> {
    let root = prepare_release(release, &release.with_file_name("wordpress-cron-port"))?;
    let code = format!(
        "define('SPEEDYSPEC_COMPAT_WP_ROOT', {:?}); define('SPEEDYSPEC_COMPAT_PORT', true); \
         define('SPEEDYSPEC_COMPAT_MULTISITE', false); require {:?};",
        format!("{}/", root.display()),
        speedyspec_compat::compat_dir()
            .join("cron/setup.php")
            .display()
            .to_string()
    );
    Ok(speedyspec_compat::PhpRunner::new(php, move |php| {
        php.eval("speedyspec://compat/cron-setup", &code)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }))
}

/// The native cron functions and their wrapper.
pub struct CronArea {
    api: Rc<CronApi>,
    hooks: Rc<WpHookApi>,
    options: Rc<OptionsApi>,
}

/// Reports nothing: a worker that does not report wake times.
struct NoWake;

impl WakeReporter for NoWake {
    fn wake_at(&self, _: i64) {}
}

fn api(hooks: &Rc<WpHookApi>, options: &Rc<OptionsApi>, wake: Rc<dyn WakeReporter>) -> Rc<CronApi> {
    Rc::new(CronApi::new(CronParts {
        hooks: hooks.clone(),
        store: Rc::new(OptionCronStore::new(options.clone())),
        keyer: Rc::new(PhpArgsKeyer),
        clock: Rc::new(SystemClock),
        translator: Rc::new(PhpTranslator),
        objects: Rc::new(PhpEventObjects),
        wake,
        doing_cron: Rc::new(PhpDoingCron),
        errors: Rc::new(PhpErrorReporter),
        network: Rc::new(PhpNetworkSites),
        switcher: Rc::new(PhpSiteSwitcher),
    }))
}

impl CronArea {
    /// The area over the worker's Hook API and options API, reporting no wake times.
    pub fn new(hooks: Rc<WpHookApi>, options: Rc<OptionsApi>) -> CronArea {
        CronArea {
            api: api(&hooks, &options, Rc::new(NoWake)),
            hooks,
            options,
        }
    }

    /// The area reporting when cron is next due to `wake` (the runtime's report to the server).
    pub fn with_wake(self, wake: Rc<dyn WakeReporter>) -> CronArea {
        CronArea {
            api: api(&self.hooks, &self.options, wake),
            hooks: self.hooks,
            options: self.options,
        }
    }

    pub fn api(&self) -> Rc<CronApi> {
        self.api.clone()
    }
}

impl NativeArea for CronArea {
    fn functions(&self) -> Vec<FunctionBuilder<'static>> {
        natives::functions(&self.api)
    }

    fn request_start(&self) {
        if let Err(err) = speedyspec_php::eval("speedyspec://wp-cron", FACADE) {
            eprintln!("speedyspec_wordpress: installing WordPress's cron functions failed: {err}");
        }
    }
}
