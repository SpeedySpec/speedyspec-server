//! WordPress's `wp_text_diff()` as a container port PHP can rebind (Facade, Adapter, Plugin).
//!
//! WordPress makes `wp_text_diff()` pluggable: a plugin replaces it by declaring it first. SpeedySpec replaces that
//! with the service container. [`TextDiffArea::bind_defaults`] binds WordPress's implementation
//! ([`WordPressTextDiffer`] over [`PhpTextDiffHost`]) under [`TEXT_DIFFER`]; [`TextDiffArea::ports`] lets a PHP object
//! implementing the twin interface (`SpeedySpec\WpTextDiff\TextDiffer`) replace it for a request, for PHP and Rust
//! callers alike.
//!
//! The facade (`text-diff.php`) declares `wp_text_diff()` at the start of every request, inside WordPress's own
//! `function_exists` guard. It calls `SpeedySpec\WpTextDiff\Native\wp_text_diff` with its arguments untouched, which
//! resolves the port on every call. WordPress's implementation parses the arguments, normalizes the whitespace and
//! renders the table in Rust; calls the native table doesn't cover (texts or titles that aren't strings, renderer
//! properties passed as arguments) go to WordPress's own classes instead, so nothing changes for them.
//!
//! `process_text_diff_html` runs through the Hook API ([`HookLineFilter`]): with no callback it is only counted,
//! as WordPress counts it, and PHP is not entered.

mod adapters;
mod filter;
mod twins;

use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_php::NativeArea;
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_php::native::{self, NativeError};
use speedyspec_wp_hooks::{HookApi, HookValue};
use speedyspec_wp_text_diff::WordPressTextDiffer;
use speedyspec_wp_text_diff::contracts::{TEXT_DIFFER, TextDiffer, TextDifferError};

pub use adapters::{PhpReplacement, PhpTextDiffHost};
pub use filter::HookLineFilter;
pub use twins::TextDifferTwin;

const FACADE: &str = include_str!("text-diff.php");

/// The native area of `wp_text_diff()`.
pub struct TextDiffArea {
    hooks: Rc<HookApi>,
    container: Rc<Container>,
}

impl TextDiffArea {
    /// The area, filtering rows through `hooks` and resolving the port from `container`.
    pub fn new(hooks: Rc<HookApi>, container: Rc<Container>) -> TextDiffArea {
        TextDiffArea { hooks, container }
    }

    /// Binds WordPress's `wp_text_diff()` under [`TEXT_DIFFER`], for the life of the worker.
    pub fn bind_defaults(&self) {
        let hooks = self.hooks.clone();
        let differ: Rc<dyn TextDiffer> =
            Rc::new(WordPressTextDiffer::new(PhpTextDiffHost::new(hooks)));
        self.container
            .bind_port::<dyn TextDiffer>(TEXT_DIFFER, move |_, _| Ok(differ.clone()));
    }

    /// `area` with the port registered, so a PHP object bound to it reaches Rust callers as a [`PhpReplacement`].
    pub fn ports(area: ContainerArea) -> ContainerArea {
        area.port::<dyn TextDiffer>(TEXT_DIFFER, |object| Rc::new(PhpReplacement::new(object)))
    }
}

/// A port error as PHP sees it.
fn native_error(err: TextDifferError) -> NativeError {
    match err {
        TextDifferError::Throw { class, message } => NativeError::throw(&class, message),
        TextDifferError::Rethrow(exception) => NativeError::Rethrow(exception),
        TextDifferError::Unwinding => NativeError::Silent,
        TextDifferError::Port(why) => NativeError::throw("Error", why),
    }
}

impl NativeArea for TextDiffArea {
    fn functions(&self) -> Vec<FunctionBuilder<'static>> {
        let container = self.container.clone();
        vec![native::function(
            "SpeedySpec\\WpTextDiff\\Native\\wp_text_diff",
            move |args| {
                let arg = |i: usize| args.get(i).cloned().unwrap_or(HookValue::Null);
                container
                    .make_port::<dyn TextDiffer>(TEXT_DIFFER)
                    .map_err(|err| NativeError::throw("Error", err.to_string()))?
                    .diff(&arg(0), &arg(1), &arg(2))
                    .map_err(native_error)
            },
        )]
    }

    fn request_start(&self) {
        if let Err(err) = speedyspec_php::eval("speedyspec://wp-text-diff", FACADE) {
            eprintln!("speedyspec_wordpress: installing wp_text_diff() failed: {err}");
        }
    }
}
