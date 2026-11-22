//! `TextApi`'s sanitizers: the cores with WordPress's filters and ports around them.

use std::borrow::Cow;
use std::cell::RefCell;

use speedyspec_hooks::Value as HookValue;
use speedyspec_wp_kses_contracts::KsesError;
use speedyspec_wp_text_contracts::TextError;

use crate::api::TextApi;
use crate::file_name::{MimeTable, default_special_chars, sanitize_file_name_core, sanitize_file_name_prepare};
use crate::helpers::antispambot_core;
use crate::sanitize::{sanitize_html_class_core, sanitize_key_core, sanitize_locale_name_core, sanitize_mime_type_core};
use crate::strings::{bytes_value, to_php_string};
use crate::title::{SanitizeContext, remove_accents, sanitize_user_core};

/// A kses failure as the text port's error, keeping it as the payload.
pub(crate) fn kses_error(e: KsesError) -> TextError {
    TextError { message: e.to_string(), payload: Some(std::rc::Rc::new(e)) }
}

/// PHP's `is_scalar()`.
fn is_scalar(value: &HookValue) -> bool {
    matches!(value, HookValue::Bool(_) | HookValue::Int(_) | HookValue::Float(_) | HookValue::Str(_) | HookValue::Bytes(_))
}

/// PHP's truthiness of a string.
fn truthy(text: &[u8]) -> bool {
    !(text.is_empty() || text == b"0")
}

impl TextApi {
    /// The locale `remove_accents()` uses when its caller gives none.
    fn locale_or(&self, locale: Option<&str>) -> Result<String, TextError> {
        match locale {
            Some(l) if !l.is_empty() => Ok(l.to_string()),
            _ => self.d.locale.locale(),
        }
    }

    /// `sanitize_key( $key )`.
    ///
    /// @param key
    ///     The key; anything but a scalar sanitizes to `""`.
    /// @return
    ///     What the `sanitize_key` filter returns (sanitized, raw).
    pub fn sanitize_key(&self, key: &HookValue) -> Result<HookValue, TextError> {
        let sanitized = match (is_scalar(key), to_php_string(key)) {
            (true, Some(text)) => sanitize_key_core(&text).into_owned(),
            _ => Vec::new(),
        };
        self.filter(&self.hook("sanitize_key"), bytes_value(sanitized), || vec![key.clone()])
    }

    /// `sanitize_html_class( $classname, $fallback )`.
    ///
    /// @param classname
    ///     The class name.
    /// @param fallback
    ///     Sanitized and returned instead when the class sanitizes to `""` and this is truthy.
    /// @return
    ///     What the `sanitize_html_class` filter returns (sanitized, class, fallback).
    pub fn sanitize_html_class(&self, classname: &[u8], fallback: &[u8]) -> Result<HookValue, TextError> {
        let sanitized = sanitize_html_class_core(classname);
        if sanitized.is_empty() && truthy(fallback) {
            return self.sanitize_html_class(fallback, b"");
        }
        self.filter(&self.hook("sanitize_html_class"), bytes_value(sanitized.into_owned()), || {
            vec![bytes_value(classname.to_vec()), bytes_value(fallback.to_vec())]
        })
    }

    /// `sanitize_locale_name( $locale_name )`.
    ///
    /// @param locale_name
    ///     The locale name.
    /// @return
    ///     What the `sanitize_locale_name` filter returns (sanitized, raw).
    pub fn sanitize_locale_name(&self, locale_name: &[u8]) -> Result<HookValue, TextError> {
        let sanitized = sanitize_locale_name_core(locale_name).into_owned();
        self.filter(&self.hook("sanitize_locale_name"), bytes_value(sanitized), || vec![bytes_value(locale_name.to_vec())])
    }

    /// `sanitize_mime_type( $mime_type )`.
    ///
    /// @param mime_type
    ///     The MIME type.
    /// @return
    ///     What the `sanitize_mime_type` filter returns (sanitized, raw).
    pub fn sanitize_mime_type(&self, mime_type: &[u8]) -> Result<HookValue, TextError> {
        let sanitized = sanitize_mime_type_core(mime_type).into_owned();
        self.filter(&self.hook("sanitize_mime_type"), bytes_value(sanitized), || vec![bytes_value(mime_type.to_vec())])
    }

    /// `remove_accents( $text, $locale )`.
    ///
    /// @param text
    ///     The text.
    /// @param locale
    ///     The locale; `None` or `""` asks the `Locale` port, as WordPress asks `get_locale()`.
    /// @return
    ///     The text without accents.
    pub fn remove_accents<'a>(&self, text: &'a [u8], locale: Option<&str>) -> Result<Cow<'a, [u8]>, TextError> {
        if text.is_ascii() {
            return Ok(Cow::Borrowed(text));
        }
        Ok(remove_accents(text, &self.locale_or(locale)?))
    }

    /// `sanitize_title( $title, $fallback_title, $context )`.
    ///
    /// @param title
    ///     The title.
    /// @param fallback
    ///     Returned when the filtered title is `""` or `false`.
    /// @param context
    ///     `Save` removes accents before the filter.
    /// @return
    ///     What the `sanitize_title` filter returns (title, raw, context), or the fallback.
    pub fn sanitize_title(&self, title: &HookValue, fallback: &[u8], context: &SanitizeContext) -> Result<HookValue, TextError> {
        let value = match (context, to_php_string(title)) {
            (SanitizeContext::Save, Some(text)) => bytes_value(self.remove_accents(&text, None)?.into_owned()),
            _ => title.clone(),
        };
        let filtered = self.filter(&self.hook("sanitize_title"), value, || {
            vec![title.clone(), bytes_value(context.as_bytes().to_vec())]
        })?;
        let empty = match &filtered {
            HookValue::Str(s) => s.is_empty(),
            HookValue::Bytes(b) => b.is_empty(),
            HookValue::Bool(false) => true,
            _ => false,
        };
        Ok(if empty { bytes_value(fallback.to_vec()) } else { filtered })
    }

    /// `sanitize_title_for_query( $title )`: `sanitize_title( $title, '', 'query' )`.
    ///
    /// @param title
    ///     The title.
    /// @return
    ///     The sanitized title.
    pub fn sanitize_title_for_query(&self, title: &HookValue) -> Result<HookValue, TextError> {
        self.sanitize_title(title, b"", &SanitizeContext::Query)
    }

    /// `sanitize_user( $username, $strict )`.
    ///
    /// @param username
    ///     The user name.
    /// @param strict
    ///     Whether to keep only ASCII letters, digits, space and `_.-@`.
    /// @return
    ///     What the `sanitize_user` filter returns (sanitized, raw, strict).
    pub fn sanitize_user(&self, username: &[u8], strict: bool) -> Result<HookValue, TextError> {
        let stripped = self.wp_strip_all_tags(&bytes_value(username.to_vec()), false)?;
        let locale = self.locale_or(None)?;
        let sanitized = sanitize_user_core(username, strict, &locale, |_| stripped.clone());
        self.filter(&self.hook("sanitize_user"), bytes_value(sanitized), || {
            vec![bytes_value(username.to_vec()), HookValue::Bool(strict)]
        })
    }

    /// `sanitize_file_name( $filename )`.
    ///
    /// @param filename
    ///     The file name.
    /// @return
    ///     What the `sanitize_file_name` filter returns (sanitized, raw).
    pub fn sanitize_file_name(&self, filename: &[u8]) -> Result<HookValue, TextError> {
        let accents = self.remove_accents(filename, None)?;
        let prepared = sanitize_file_name_prepare(&accents);
        let raw = || bytes_value(filename.to_vec());
        let special_hook = self.hook("sanitize_file_name_chars");
        let special = match self.filter(&special_hook, bytes_list(default_special_chars()), || vec![raw()])? {
            HookValue::Array(items) => items.iter().filter_map(|(_, v)| to_php_string(v)).collect(),
            HookValue::Null => Vec::new(),
            scalar => to_php_string(&scalar).into_iter().collect(),
        };
        let mimes = MimeTable(self.d.mime_types.mime_types()?);
        let allowed = MimeTable(self.d.mime_types.allowed_mime_types()?);
        let sanitized = sanitize_file_name_core(&prepared, &special, &mimes, &allowed);
        self.filter(&self.hook("sanitize_file_name"), bytes_value(sanitized), || vec![raw()])
    }

    /// `antispambot( $email_address, $hex_encoding )`.
    ///
    /// @param email
    ///     The address.
    /// @param hex_encoding
    ///     1 to allow percent-encoded characters.
    /// @return
    ///     The obfuscated address.
    pub fn antispambot(&self, email: &[u8], hex_encoding: i64) -> Result<Vec<u8>, TextError> {
        let failure: RefCell<Option<TextError>> = RefCell::new(None);
        let mut rand = |min: i64, max: i64| match self.d.random.rand(min, max) {
            Ok(n) => n,
            Err(e) => {
                failure.borrow_mut().get_or_insert(e);
                1
            }
        };
        let out = antispambot_core(email, hex_encoding, &mut rand);
        match failure.into_inner() {
            Some(e) => Err(e),
            None => Ok(out),
        }
    }
}

/// A PHP list of strings.
fn bytes_list(items: Vec<Vec<u8>>) -> HookValue {
    HookValue::Array(
        items.into_iter().enumerate().map(|(i, b)| (speedyspec_hooks::Key::Int(i as i64), bytes_value(b))).collect(),
    )
}
