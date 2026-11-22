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

/// PHP's `empty()`.
fn php_empty(value: &HookValue) -> bool {
    match value {
        HookValue::Null | HookValue::Bool(false) => true,
        HookValue::Bool(true) => false,
        HookValue::Int(i) => *i == 0,
        HookValue::Float(f) => *f == 0.0,
        HookValue::Str(s) => s.is_empty() || s == "0",
        HookValue::Bytes(b) => b.is_empty() || b == b"0",
        HookValue::Array(a) => a.is_empty(),
        HookValue::Opaque(_) => false,
    }
}

/// A value WordPress reads as a string; an array or object is an error here (PHP throws or warns).
fn string_arg(value: &HookValue, what: &str) -> Result<Vec<u8>, TextError> {
    to_php_string(value).ok_or_else(|| TextError::message(format!("{what}: expected a string, got an array or object")))
}

/// `_sanitize_text_fields()`'s and `sanitize_trackback_urls()`'s patterns.
struct FieldPatterns {
    less_than: crate::preg::Pattern,
    whitespace: crate::preg::Pattern,
    octet: crate::preg::Pattern,
    spaces: crate::preg::Pattern,
    trackback_split: crate::preg::Pattern,
    trackback_url: crate::preg::Pattern,
}

static FIELDS: std::sync::LazyLock<FieldPatterns> = std::sync::LazyLock::new(|| {
    use crate::preg::Pattern;
    FieldPatterns {
        less_than: Pattern::php("%<[^>]*?((?=<)|>|$)%"),
        whitespace: Pattern::php("/[\\r\\n\\t ]+/"),
        octet: Pattern::php("/%[a-f0-9]{2}/i"),
        spaces: Pattern::php("/ +/"),
        trackback_split: Pattern::php("/[\\r\\n\\t ]/"),
        trackback_url: Pattern::php("#^https?://.#i"),
    }
});

impl TextApi {
    /// `esc_url( $url, $protocols, $_context )`.
    ///
    /// @param url
    ///     The URL.
    /// @param protocols
    ///     The protocols to keep when the caller passes an array; `None` is `wp_allowed_protocols()`.
    /// @param context
    ///     `display` also encodes `&` and `'`; anything else (`db`) does not.
    /// @return
    ///     `""` for `""` (no filter), otherwise what the `clean_url` filter returns (clean URL, original, context).
    pub fn esc_url(&self, url: &[u8], protocols: Option<&speedyspec_wp_kses_contracts::Protocols>, context: &[u8]) -> Result<HookValue, TextError> {
        if url.is_empty() {
            return Ok(HookValue::Str(String::new()));
        }
        let globals = &self.d.kses.deps().globals;
        let (html, xml) = (globals.entity_names(), globals.xml_entity_names());
        let good = crate::esc_url::esc_url_core(
            url,
            protocols,
            || self.d.kses.allowed_protocols().map_err(kses_error),
            context == b"display",
            (&html, &xml),
        )?;
        self.filter(&self.hook("clean_url"), bytes_value(good), || {
            vec![bytes_value(url.to_vec()), bytes_value(context.to_vec())]
        })
    }

    /// `esc_url_raw( $url, $protocols )`: `sanitize_url()`.
    ///
    /// @param url
    ///     The URL.
    /// @param protocols
    ///     As for [`TextApi::esc_url`].
    /// @return
    ///     The URL cleaned for storage.
    pub fn esc_url_raw(&self, url: &[u8], protocols: Option<&speedyspec_wp_kses_contracts::Protocols>) -> Result<HookValue, TextError> {
        self.sanitize_url(url, protocols)
    }

    /// `sanitize_url( $url, $protocols )`: `esc_url()` in the `db` context.
    ///
    /// @param url
    ///     The URL.
    /// @param protocols
    ///     As for [`TextApi::esc_url`].
    /// @return
    ///     The URL cleaned for storage.
    pub fn sanitize_url(&self, url: &[u8], protocols: Option<&speedyspec_wp_kses_contracts::Protocols>) -> Result<HookValue, TextError> {
        self.esc_url(url, protocols, b"db")
    }

    /// `wp_pre_kses_less_than( $content )`: a `<` that starts no tag is escaped (through `esc_html()`, so its filter
    /// fires).
    ///
    /// @param content
    ///     The text.
    /// @return
    ///     The text with lone `<` escaped.
    pub fn wp_pre_kses_less_than(&self, content: &[u8]) -> Result<Vec<u8>, TextError> {
        let failure: RefCell<Option<TextError>> = RefCell::new(None);
        let out = crate::preg::preg_replace_callback(&FIELDS.less_than, content, |m| {
            let whole = m.get(0).unwrap_or_default();
            if whole.contains(&b'>') {
                return whole.to_vec();
            }
            match self.d.escape.esc_html(whole).map(|v| to_php_string(&v).unwrap_or_default()) {
                Ok(escaped) => escaped,
                Err(e) => {
                    failure.borrow_mut().get_or_insert(e);
                    Vec::new()
                }
            }
        });
        if let Some(e) = failure.into_inner() {
            return Err(e);
        }
        // PHP's preg_replace_callback returns null on a PCRE failure, which callers read as "".
        Ok(out.map_or_else(|_| Vec::new(), std::borrow::Cow::into_owned))
    }

    /// `_sanitize_text_fields( $str, $keep_newlines )`.
    fn sanitize_text_fields(&self, value: &HookValue, keep_newlines: bool) -> Result<Vec<u8>, TextError> {
        let Some(text) = (match value {
            HookValue::Array(_) | HookValue::Opaque(_) => None,
            other => to_php_string(other),
        }) else {
            return Ok(Vec::new());
        };
        let mut filtered = self.d.escape.check_invalid_utf8(&text, false)?;
        if filtered.contains(&b'<') {
            let escaped = self.wp_pre_kses_less_than(&filtered)?;
            let stripped = self.wp_strip_all_tags(&bytes_value(escaped), false)?;
            filtered = crate::strings::replace_one(&stripped, b"<\n", b"&lt;\n").into_owned();
        }
        let replace = |p: &crate::preg::Pattern, s: &[u8], with: &[u8]| {
            crate::preg::preg_replace(p, s, with, None).map_or_else(|_| Vec::new(), |(o, _)| o.into_owned())
        };
        if !keep_newlines {
            filtered = replace(&FIELDS.whitespace, &filtered, b" ");
        }
        let mut filtered = crate::strings::php_trim(&filtered, crate::strings::PHP_TRIM_DEFAULT).to_vec();
        let mut found = false;
        while let Ok(Some(m)) = crate::preg::preg_match(&FIELDS.octet, &filtered) {
            let octet = m.get(0).unwrap_or_default().to_vec();
            filtered = crate::strings::replace_one(&filtered, &octet, b"").into_owned();
            found = true;
        }
        if found {
            let collapsed = replace(&FIELDS.spaces, &filtered, b" ");
            filtered = crate::strings::php_trim(&collapsed, crate::strings::PHP_TRIM_DEFAULT).to_vec();
        }
        Ok(filtered)
    }

    /// `sanitize_text_field( $str )`.
    ///
    /// @param value
    ///     The value; an array or object sanitizes to `""`.
    /// @return
    ///     What the `sanitize_text_field` filter returns (sanitized, raw).
    pub fn sanitize_text_field(&self, value: &HookValue) -> Result<HookValue, TextError> {
        let filtered = self.sanitize_text_fields(value, false)?;
        self.filter(&self.hook("sanitize_text_field"), bytes_value(filtered), || vec![value.clone()])
    }

    /// `sanitize_textarea_field( $str )`: as `sanitize_text_field()`, keeping newlines.
    ///
    /// @param value
    ///     The value.
    /// @return
    ///     What the `sanitize_textarea_field` filter returns (sanitized, raw).
    pub fn sanitize_textarea_field(&self, value: &HookValue) -> Result<HookValue, TextError> {
        let filtered = self.sanitize_text_fields(value, true)?;
        self.filter(&self.hook("sanitize_textarea_field"), bytes_value(filtered), || vec![value.clone()])
    }

    /// `format_to_edit( $content, $rich_text )`.
    ///
    /// @param content
    ///     The content.
    /// @param rich_text
    ///     Whether the content is for a rich editor; otherwise it is `esc_textarea()`d after the filter.
    /// @return
    ///     The filtered (and maybe escaped) content.
    pub fn format_to_edit(&self, content: &[u8], rich_text: bool) -> Result<HookValue, TextError> {
        let filtered = self.filter(&self.hook("format_to_edit"), bytes_value(content.to_vec()), Vec::new)?;
        if rich_text {
            return Ok(filtered);
        }
        self.d.escape.esc_textarea(&string_arg(&filtered, "format_to_edit")?)
    }

    /// `is_email( $email, $deprecated )`.
    ///
    /// @param email
    ///     The address.
    /// @param deprecated
    ///     Reported through `_deprecated_argument()` when not empty.
    /// @return
    ///     What the `is_email` filter returns: `false` (value, address, context) or the address (address, address,
    ///     `null`).
    pub fn is_email(&self, email: &HookValue, deprecated: &HookValue) -> Result<HookValue, TextError> {
        if !php_empty(deprecated) {
            self.d.diagnostics.deprecated_argument("is_email", "3.0.0", "")?;
        }
        let text = string_arg(email, "is_email")?;
        let (value, context) = match crate::email::is_email_core(&text) {
            crate::email::EmailCheck::Valid(_) => (email.clone(), HookValue::Null),
            crate::email::EmailCheck::Invalid(context) => (HookValue::Bool(false), HookValue::Str(context.into())),
        };
        self.filter(&self.hook("is_email"), value, || vec![email.clone(), context])
    }

    /// `sanitize_email( $email )`.
    ///
    /// @param email
    ///     The address, already a string.
    /// @return
    ///     What the `sanitize_email` filter returns (sanitized or `""`, raw, context or `null`).
    pub fn sanitize_email(&self, email: &[u8]) -> Result<HookValue, TextError> {
        let (sanitized, context) = crate::email::sanitize_email_core(email);
        let context = context.map_or(HookValue::Null, |c| HookValue::Str(c.into()));
        self.filter(&self.hook("sanitize_email"), bytes_value(sanitized), || vec![bytes_value(email.to_vec()), context])
    }

    /// `wp_parse_str( $input_string, $result )`'s `$result`.
    ///
    /// @param input
    ///     The query string (WordPress casts it with `(string)`).
    /// @return
    ///     What the `wp_parse_str` filter returns.
    pub fn wp_parse_str(&self, input: &HookValue) -> Result<HookValue, TextError> {
        let text = to_php_string(input).unwrap_or_else(|| b"Array".to_vec());
        self.filter(&self.hook("wp_parse_str"), crate::parse_str::parse_str(&text), Vec::new)
    }

    /// `sanitize_trackback_urls( $to_ping )`.
    ///
    /// @param to_ping
    ///     Whitespace-separated URLs.
    /// @return
    ///     What the `sanitize_trackback_urls` filter returns: the `http(s)` URLs, each through `sanitize_url()`, one
    ///     per line (sanitized, raw).
    pub fn sanitize_trackback_urls(&self, to_ping: &[u8]) -> Result<HookValue, TextError> {
        let trimmed = crate::strings::php_trim(to_ping, crate::strings::PHP_TRIM_DEFAULT);
        let flags = crate::preg::SplitFlags { delim_capture: false, no_empty: true };
        let pieces = crate::preg::preg_split(&FIELDS.trackback_split, trimmed, -1, flags).unwrap_or_default();
        let mut urls = Vec::new();
        for url in pieces {
            if matches!(crate::preg::preg_match(&FIELDS.trackback_url, url), Ok(Some(_))) {
                urls.push(string_arg(&self.sanitize_url(url, None)?, "sanitize_trackback_urls")?);
            }
        }
        self.filter(&self.hook("sanitize_trackback_urls"), bytes_value(urls.join(&b'\n')), || {
            vec![bytes_value(to_ping.to_vec())]
        })
    }
}
