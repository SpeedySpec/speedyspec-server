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
use speedyspec_wp_cron_options::OptionCronStore;
use speedyspec_wp_hooks::HookApi;
use speedyspec_wp_options::OptionsApi;

pub use adapters::{PhpArgsKeyer, PhpEventObjects, PhpTranslator, SystemClock};
pub use release::{PORTED_FUNCTIONS, prepare_release, strip_ported};

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
}

impl CronArea {
    /// The area over the worker's Hook API and options API.
    pub fn new(hooks: Rc<HookApi>, options: Rc<OptionsApi>) -> CronArea {
        CronArea {
            api: Rc::new(CronApi::new(CronParts {
                hooks,
                store: Rc::new(OptionCronStore::new(options)),
                keyer: Rc::new(PhpArgsKeyer),
                clock: Rc::new(SystemClock),
                translator: Rc::new(PhpTranslator),
                objects: Rc::new(PhpEventObjects),
                // Replaced with the runtime's slot, the doing_cron transient and error_log() in the next tasks.
                wake: Rc::new(speedyspec_wp_cron::memory::RecordingWake::new()),
                doing_cron: Rc::new(speedyspec_wp_cron::memory::MemoryDoingCron::new()),
                errors: Rc::new(speedyspec_wp_cron::memory::CollectingErrors::new()),
            })),
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
