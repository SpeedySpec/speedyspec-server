//! The ports the engine runs with, bundled.

use std::rc::Rc;

use speedyspec_wp_theme_json_contracts::ports::{
    ActiveTheme, BlockMetadata, Diagnostics, Gettext, GlobalSettings, SafeCss, TextFunctions,
    ThemeJsonFilters,
};

use crate::filters::NoFilters;
use crate::memory::{MemoryGlobalSettings, NoThemeSupports};

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
    /// kses's `safecss_filter_attr()`.
    pub safe_css: Rc<dyn SafeCss>,
    /// The site's merged settings fluid typography reads (`wp_get_global_settings()`).
    pub global_settings: Rc<dyn GlobalSettings>,
    /// The filters `WP_Theme_JSON` applies.
    pub filters: Rc<dyn ThemeJsonFilters>,
    /// The active theme (`current_theme_supports()`).
    pub theme: Rc<dyn ActiveTheme>,
}

impl ThemeJsonDeps {
    /// The ports, with no site settings, no filters and a theme that supports nothing; set those with the
    /// `with_*` methods.
    pub fn new(
        blocks: Rc<dyn BlockMetadata>,
        text: Rc<dyn TextFunctions>,
        gettext: Rc<dyn Gettext>,
        diagnostics: Rc<dyn Diagnostics>,
        safe_css: Rc<dyn SafeCss>,
    ) -> ThemeJsonDeps {
        ThemeJsonDeps {
            blocks,
            text,
            gettext,
            diagnostics,
            safe_css,
            global_settings: Rc::new(MemoryGlobalSettings::default()),
            filters: Rc::new(NoFilters),
            theme: Rc::new(NoThemeSupports),
        }
    }

    /// With these site settings.
    pub fn with_global_settings(
        mut self,
        global_settings: Rc<dyn GlobalSettings>,
    ) -> ThemeJsonDeps {
        self.global_settings = global_settings;
        self
    }

    /// With these filters.
    pub fn with_filters(mut self, filters: Rc<dyn ThemeJsonFilters>) -> ThemeJsonDeps {
        self.filters = filters;
        self
    }

    /// With this active theme.
    pub fn with_theme(mut self, theme: Rc<dyn ActiveTheme>) -> ThemeJsonDeps {
        self.theme = theme;
        self
    }
}
