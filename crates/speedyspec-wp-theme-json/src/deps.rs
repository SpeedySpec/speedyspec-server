//! The ports the engine runs with, bundled.

use std::rc::Rc;

use speedyspec_wp_theme_json_contracts::ports::{BlockMetadata, Diagnostics, Gettext, TextFunctions};

/// What the theme.json engine needs from outside.
#[derive(Clone)]
pub struct ThemeJsonDeps {
    /// The registered blocks and block styles.
    pub blocks: Rc<dyn BlockMetadata>,
    /// The text functions (`sanitize_title`, `esc_html`, …).
    pub text: Rc<dyn TextFunctions>,
    /// WordPress's translations (`__()`).
    pub gettext: Rc<dyn Gettext>,
    /// WordPress's developer notices.
    pub diagnostics: Rc<dyn Diagnostics>,
}
