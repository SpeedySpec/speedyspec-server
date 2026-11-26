//! `EscapeApi`: WordPress's escaping functions with their filters and settings around the cores.

use std::cell::Cell;
use std::rc::Rc;

use speedyspec_hooks::Value as HookValue;
use speedyspec_wp_hooks::WpHookApi;
use speedyspec_wp_hooks_contracts::{Count, HookError, HookName};
use speedyspec_wp_text_contracts::{Diagnostics, SiteSettings, TextError};

use crate::charset::Charset;
use crate::decode::QuoteStyle;
use crate::htmlspecialchars::{EntFlags, htmlspecialchars};
use crate::js::esc_js_after_specialchars;
use crate::legacy::seems_utf8_core;
use crate::slash::{php_string, wp_slash_value};
use crate::specialchars::{check_invalid_utf8_for, wp_specialchars};
use crate::tag::tag_escape_core;
use crate::xml::esc_xml_after_check;

/// A `HookError` as the text port's error.
///
/// @param e
///     The hook error.
/// @return
///     The same message as a `TextError`, keeping the hook error's payload (a PHP exception object, for the PHP
///     surface to rethrow).
pub fn text_error(e: HookError) -> TextError {
    TextError { message: e.message, payload: e.payload }
}

/// The hook names `EscapeApi` fires, hashed once.
struct Names {
    esc_html: HookName,
    attribute_escape: HookName,
    esc_textarea: HookName,
    esc_xml: HookName,
    js_escape: HookName,
    tag_escape: HookName,
}

/// The facade over the escaping functions: one method per WordPress function that fires a filter, reads a setting
/// or reports a deprecation.
///
/// Functions with none of those (`wp_specialchars_decode`, `htmlentities2`, `backslashit`, the slashing functions)
/// have no method; call their cores directly.
pub struct EscapeApi {
    hooks: Rc<WpHookApi>,
    settings: Rc<dyn SiteSettings>,
    diagnostics: Rc<dyn Diagnostics>,
    /// `wp_check_invalid_utf8()`'s `static $is_utf8`: read once, never refreshed.
    is_utf8: Cell<Option<bool>>,
    names: Names,
}

impl EscapeApi {
    /// Forgets what WordPress keeps for one request in function statics (`wp_check_invalid_utf8()`'s `$is_utf8`), so
    /// the next request reads `blog_charset` again.
    pub fn new_request(&self) {
        self.is_utf8.set(None);
    }

    /// A facade on the hooks and ports.
    ///
    /// @param hooks
    ///     Where the filters fire.
    /// @param settings
    ///     `blog_charset`.
    /// @param diagnostics
    ///     Where deprecations are reported.
    /// @return
    ///     The facade.
    pub fn new(hooks: Rc<WpHookApi>, settings: Rc<dyn SiteSettings>, diagnostics: Rc<dyn Diagnostics>) -> EscapeApi {
        EscapeApi {
            hooks,
            settings,
            diagnostics,
            is_utf8: Cell::new(None),
            names: Names {
                esc_html: HookName::from("esc_html"),
                attribute_escape: HookName::from("attribute_escape"),
                esc_textarea: HookName::from("esc_textarea"),
                esc_xml: HookName::from("esc_xml"),
                js_escape: HookName::from("js_escape"),
                tag_escape: HookName::from("tag_escape"),
            },
        }
    }

    /// `apply_filters( $hook, $safe, $original )`, through the fast path when nothing listens.
    fn filter(&self, name: &HookName, safe: Vec<u8>, original: &[u8]) -> Result<HookValue, TextError> {
        let safe = php_string(safe);
        if self.hooks.skip_if_empty(name, Count::Filters).map_err(text_error)? {
            return Ok(safe);
        }
        let out = self.hooks.apply_filters(name, safe, vec![php_string(original.to_vec())]).map_err(text_error)?;
        Ok(out.read().into_owned())
    }

    fn is_utf8(&self) -> Result<bool, TextError> {
        if let Some(is_utf8) = self.is_utf8.get() {
            return Ok(is_utf8);
        }
        let is_utf8 = speedyspec_wp_utf8::is_utf8_charset(self.settings.blog_charset()?.as_bytes());
        self.is_utf8.set(Some(is_utf8));
        Ok(is_utf8)
    }

    /// `_wp_specialchars( $text, $quote_style, $charset, $double_encode )`.
    ///
    /// @param text
    ///     The text.
    /// @param style
    ///     The quote style.
    /// @param charset
    ///     The charset; `None` reads `blog_charset`.
    /// @param double_encode
    ///     Whether existing references are escaped again.
    /// @return
    ///     The escaped text.
    pub fn specialchars(
        &self,
        text: &[u8],
        style: QuoteStyle,
        charset: Option<&str>,
        double_encode: bool,
    ) -> Result<Vec<u8>, TextError> {
        if text.is_empty() || !text.iter().any(|b| matches!(b, b'&' | b'<' | b'>' | b'"' | b'\'')) {
            return Ok(text.to_vec());
        }
        let blog_charset;
        let charset = match charset {
            Some(charset) if !charset.is_empty() && charset != "0" => charset,
            _ => {
                blog_charset = self.settings.blog_charset()?;
                &blog_charset
            }
        };
        Ok(wp_specialchars(text, style, charset, double_encode).into_owned())
    }

    /// `wp_check_invalid_utf8( $text, $strip )`.
    ///
    /// @param text
    ///     The text.
    /// @param strip
    ///     On invalid UTF-8, scrub instead of returning `""`.
    /// @return
    ///     The text, `""` or the scrubbed text.
    pub fn check_invalid_utf8(&self, text: &[u8], strip: bool) -> Result<Vec<u8>, TextError> {
        if text.is_empty() {
            return Ok(Vec::new());
        }
        Ok(check_invalid_utf8_for(text, self.is_utf8()?, strip).into_owned())
    }

    /// `wp_check_invalid_utf8()` then `_wp_specialchars( ..., ENT_QUOTES )`, as `esc_html()` and `esc_attr()` do.
    fn quoted(&self, text: &[u8]) -> Result<Vec<u8>, TextError> {
        let checked = self.check_invalid_utf8(text, false)?;
        self.specialchars(&checked, QuoteStyle::Quotes, None, false)
    }

    /// `esc_html( $text )`, firing `esc_html`.
    ///
    /// @param text
    ///     The text.
    /// @return
    ///     What the filter returns.
    pub fn esc_html(&self, text: &[u8]) -> Result<HookValue, TextError> {
        let safe = self.quoted(text)?;
        self.filter(&self.names.esc_html, safe, text)
    }

    /// `esc_attr( $text )`, firing `attribute_escape`.
    ///
    /// @param text
    ///     The text.
    /// @return
    ///     What the filter returns.
    pub fn esc_attr(&self, text: &[u8]) -> Result<HookValue, TextError> {
        let safe = self.quoted(text)?;
        self.filter(&self.names.attribute_escape, safe, text)
    }

    /// `esc_textarea( $text )`, firing `esc_textarea`: PHP's `htmlspecialchars()` with `ENT_QUOTES` and
    /// `blog_charset` as it is (not canonicalized), double-encoding.
    ///
    /// @param text
    ///     The text.
    /// @return
    ///     What the filter returns.
    pub fn esc_textarea(&self, text: &[u8]) -> Result<HookValue, TextError> {
        let charset = Charset::from_php_name(&self.settings.blog_charset()?);
        let safe = htmlspecialchars(text, EntFlags::from_php(3), &charset, true).into_owned();
        self.filter(&self.names.esc_textarea, safe, text)
    }

    /// `esc_xml( $text )`, firing `esc_xml`.
    ///
    /// @param text
    ///     The text.
    /// @return
    ///     What the filter returns.
    pub fn esc_xml(&self, text: &[u8]) -> Result<HookValue, TextError> {
        let checked = self.check_invalid_utf8(text, false)?;
        let safe = esc_xml_after_check(&checked, &self.settings.blog_charset()?);
        self.filter(&self.names.esc_xml, safe, text)
    }

    /// `esc_js( $text )`, firing `js_escape`.
    ///
    /// @param text
    ///     The text.
    /// @return
    ///     What the filter returns.
    pub fn esc_js(&self, text: &[u8]) -> Result<HookValue, TextError> {
        let checked = self.check_invalid_utf8(text, false)?;
        let escaped = self.specialchars(&checked, QuoteStyle::Double, None, false)?;
        let safe = esc_js_after_specialchars(&escaped);
        self.filter(&self.names.js_escape, safe, text)
    }

    /// `tag_escape( $tag_name )`, firing `tag_escape`.
    ///
    /// @param text
    ///     The tag name.
    /// @return
    ///     What the filter returns.
    pub fn tag_escape(&self, text: &[u8]) -> Result<HookValue, TextError> {
        let safe = tag_escape_core(text).into_owned();
        self.filter(&self.names.tag_escape, safe, text)
    }

    /// `seems_utf8( $str )`, reporting its deprecation.
    ///
    /// @param text
    ///     The bytes.
    /// @return
    ///     Whether they look like UTF-8 by WordPress's old test.
    pub fn seems_utf8(&self, text: &[u8]) -> Result<bool, TextError> {
        self.diagnostics.deprecated_function("seems_utf8", "6.9.0", "wp_is_valid_utf8()")?;
        Ok(seems_utf8_core(text))
    }

    /// `addslashes_gpc( $gpc )`, reporting its deprecation.
    ///
    /// @param value
    ///     A string or a nested array of strings.
    /// @return
    ///     `wp_slash( $gpc )`.
    pub fn addslashes_gpc(&self, value: &HookValue) -> Result<HookValue, TextError> {
        self.diagnostics.deprecated_function("addslashes_gpc", "7.0.0", "wp_slash()")?;
        Ok(wp_slash_value(value))
    }
}
