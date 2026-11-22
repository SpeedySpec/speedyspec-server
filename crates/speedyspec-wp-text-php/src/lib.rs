//! WordPress's formatting functions as native PHP functions, over the Rust text port.
//!
//! - [`TextArea`] builds `EscapeApi` and `TextApi` over PHP adapters ([`adapters`]), binds their use cases in the
//!   container, and registers one native function per ported WordPress function, under WordPress's own name.
//! - Filters fire through the shared `HookApi`, so PHP and Rust callbacks interleave by priority, as in WordPress.
//! - What WordPress keeps in function statics lasts one request: [`TextArea`] resets it at each request start.

pub mod adapters;
mod args;
mod native;
mod release;


use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_php::NativeArea;
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_wp_hooks::HookApi;
use speedyspec_wp_kses::KsesApi;
use speedyspec_wp_text::{TextApi, TextDeps, register_text_handlers};
use speedyspec_wp_text_escape::{EscapeApi, register_escape_handlers};

pub use release::{PORTED_FUNCTIONS, prepare_release, strip_ported};
pub use speedyspec_wp_text::adapters::EscapeAttributes;

use crate::adapters::*;

/// The PHP declaration of the one native with a by-reference parameter (`wp_parse_str`), evaluated at the start of
/// every request.
pub const FACADE: &str = include_str!("formatting.php");

/// The WordPress functions this crate provides natively, in registration order.
///
/// @return
///     Their names.
pub fn native_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = native::entries().into_iter().map(|(name, ..)| name).collect();
    names.push("wp_parse_str");
    names
}

/// The native area of WordPress's formatting functions.
pub struct TextArea {
    api: Rc<TextApi>,
    escape: Rc<EscapeApi>,
}

impl TextArea {
    /// The formatting APIs over `hooks` and `kses` with the PHP adapters, and their use cases bound in `container`.
    ///
    /// @param container
    ///     Where the use cases are bound.
    /// @param hooks
    ///     The shared hook store's API.
    /// @param kses
    ///     kses, for `wp_allowed_protocols()`, the URI attributes and the entity names.
    /// @return
    ///     The area.
    pub fn new(container: Rc<Container>, hooks: Rc<HookApi>, kses: Rc<KsesApi>) -> TextArea {
        let settings = Rc::new(PhpSiteSettings);
        let diagnostics = Rc::new(PhpDiagnostics);
        let escape = Rc::new(EscapeApi::new(hooks.clone(), settings.clone(), diagnostics.clone()));
        let api = Rc::new(TextApi::new(TextDeps {
            hooks,
            settings,
            translator: Rc::new(PhpTranslator),
            diagnostics,
            texturize_overrides: Rc::new(PhpTexturizeOverrides),
            shortcode_tags: Rc::new(PhpShortcodeTags),
            escape: escape.clone(),
            kses,
            locale: Rc::new(PhpLocale),
            mime_types: Rc::new(PhpMimeTypes),
            random: Rc::new(PhpRandom),
            smilies: Rc::new(PhpSmilies),
            link_context: Rc::new(PhpLinkContext),
        }));
        register_escape_handlers(&container, escape.clone());
        register_text_handlers(&container, api.clone());
        let shared = Rc::new(native::Shared::new(container));
        native::SHARED.with(|s| *s.borrow_mut() = Some(shared));
        TextArea { api, escape }
    }

    /// The formatting API for Rust callers.
    pub fn api(&self) -> Rc<TextApi> {
        self.api.clone()
    }

    /// The escaping API for Rust callers.
    pub fn escape(&self) -> Rc<EscapeApi> {
        self.escape.clone()
    }
}

impl NativeArea for TextArea {
    fn functions(&self) -> Vec<FunctionBuilder<'static>> {
        native::functions()
    }

    fn request_start(&self) {
        self.api.new_request();
        self.escape.new_request();
        if let Err(err) = speedyspec_php::eval("speedyspec://wp-text", FACADE) {
            eprintln!("speedyspec_wordpress: installing the formatting functions failed: {err}");
        }
    }

    fn request_end(&self) {
        self.api.new_request();
        self.escape.new_request();
    }
}
