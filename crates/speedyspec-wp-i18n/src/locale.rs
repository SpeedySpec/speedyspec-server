//! `WP_Locale`'s strings: the lookups `WP_Locale::init()` makes, in its order, and the values it derives from them.
//!
//! The order is observable: each lookup fires the `gettext` (or `gettext_with_context`) filters. The PHP class
//! assigns [`locale_strings`]'s values with WordPress's own expressions (array keys, `str_replace`, `===`), so PHP's
//! semantics hold there; [`LocaleData`] is the same derivation for Rust callers.

use speedyspec_wp_i18n_contracts::{LocaleStrings, PhpValue, PortError, WordCountType};

/// `WP_Locale::init()`'s lookups before the text direction: `(text, context)`, in WordPress 7.1.5's order. Weekdays
/// (7), their initials (7) and abbreviations (7), months (12), genitive months (12), month abbreviations (12),
/// meridiems (4), the two number format strings and the list item separator.
pub const LOCALE_STRINGS: &[(&str, Option<&str>)] = &[
    ("Sunday", None),
    ("Monday", None),
    ("Tuesday", None),
    ("Wednesday", None),
    ("Thursday", None),
    ("Friday", None),
    ("Saturday", None),
    ("S", Some("Sunday initial")),
    ("M", Some("Monday initial")),
    ("T", Some("Tuesday initial")),
    ("W", Some("Wednesday initial")),
    ("T", Some("Thursday initial")),
    ("F", Some("Friday initial")),
    ("S", Some("Saturday initial")),
    ("Sun", None),
    ("Mon", None),
    ("Tue", None),
    ("Wed", None),
    ("Thu", None),
    ("Fri", None),
    ("Sat", None),
    ("January", None),
    ("February", None),
    ("March", None),
    ("April", None),
    ("May", None),
    ("June", None),
    ("July", None),
    ("August", None),
    ("September", None),
    ("October", None),
    ("November", None),
    ("December", None),
    ("January", Some("genitive")),
    ("February", Some("genitive")),
    ("March", Some("genitive")),
    ("April", Some("genitive")),
    ("May", Some("genitive")),
    ("June", Some("genitive")),
    ("July", Some("genitive")),
    ("August", Some("genitive")),
    ("September", Some("genitive")),
    ("October", Some("genitive")),
    ("November", Some("genitive")),
    ("December", Some("genitive")),
    ("Jan", Some("January abbreviation")),
    ("Feb", Some("February abbreviation")),
    ("Mar", Some("March abbreviation")),
    ("Apr", Some("April abbreviation")),
    ("May", Some("May abbreviation")),
    ("Jun", Some("June abbreviation")),
    ("Jul", Some("July abbreviation")),
    ("Aug", Some("August abbreviation")),
    ("Sep", Some("September abbreviation")),
    ("Oct", Some("October abbreviation")),
    ("Nov", Some("November abbreviation")),
    ("Dec", Some("December abbreviation")),
    ("am", None),
    ("pm", None),
    ("AM", None),
    ("PM", None),
    ("number_format_thousands_sep", None),
    ("number_format_decimal_point", None),
    (", ", None),
];

/// The text direction lookup, made only when `$GLOBALS['text_direction']` is not set.
pub const TEXT_DIRECTION: (&str, &str) = ("ltr", "text direction");

/// The word count type lookup (`WP_Locale::get_word_count_type()`).
pub const WORD_COUNT_TYPE: (&str, &str) = ("words", "Word count type. Do not translate!");

fn lookup(
    strings: &impl LocaleStrings,
    text: &str,
    context: Option<&str>,
) -> Result<PhpValue, PortError> {
    match context {
        Some(c) => strings.translate_with_context(text, c),
        None => strings.translate(text),
    }
}

/// Every [`LOCALE_STRINGS`] lookup, in order, then the text direction lookup when `ask_text_direction`.
pub fn locale_strings(
    strings: &impl LocaleStrings,
    ask_text_direction: bool,
) -> Result<Vec<PhpValue>, PortError> {
    let mut out = Vec::with_capacity(LOCALE_STRINGS.len() + 1);
    for (text, context) in LOCALE_STRINGS {
        out.push(lookup(strings, text, *context)?);
    }
    if ask_text_direction {
        out.push(strings.translate_with_context(TEXT_DIRECTION.0, TEXT_DIRECTION.1)?);
    }
    Ok(out)
}

/// A value as PHP would turn it into a string, for the cases translations produce (a filter may return anything).
fn bytes(value: &PhpValue) -> Vec<u8> {
    match value {
        PhpValue::Str(s) => s.as_bytes().to_vec(),
        PhpValue::Bytes(b) => b.clone(),
        PhpValue::Int(n) => n.to_string().into_bytes(),
        PhpValue::Bool(true) => b"1".to_vec(),
        PhpValue::Float(f) => f.to_string().into_bytes(),
        PhpValue::Null | PhpValue::Bool(false) => Vec::new(),
        PhpValue::Array(_) => b"Array".to_vec(),
        PhpValue::Opaque(_) => Vec::new(),
    }
}

/// `WP_Locale`'s data, for Rust callers. Pairs keep WordPress's key order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocaleData {
    /// Weekday names, Sunday first.
    pub weekday: Vec<Vec<u8>>,
    /// One-letter weekday abbreviations, keyed by weekday name.
    pub weekday_initial: Vec<(Vec<u8>, Vec<u8>)>,
    /// Three-letter weekday abbreviations, keyed by weekday name.
    pub weekday_abbrev: Vec<(Vec<u8>, Vec<u8>)>,
    /// Month names, keyed `01` to `12`.
    pub month: Vec<(String, Vec<u8>)>,
    /// Genitive month names, keyed `01` to `12`.
    pub month_genitive: Vec<(String, Vec<u8>)>,
    /// Three-letter month abbreviations, keyed by month name.
    pub month_abbrev: Vec<(Vec<u8>, Vec<u8>)>,
    /// `am`, `pm`, `AM` and `PM`, keyed by themselves.
    pub meridiem: Vec<(&'static str, Vec<u8>)>,
    /// `number_format()`'s thousands separator (`,` when untranslated; spaces become `&nbsp;`).
    pub thousands_sep: Vec<u8>,
    /// `number_format()`'s decimal point (`.` when untranslated).
    pub decimal_point: Vec<u8>,
    /// The separator between list items.
    pub list_item_separator: Vec<u8>,
    /// `ltr` or `rtl` (or the `$GLOBALS['text_direction']` given).
    pub text_direction: Vec<u8>,
    /// How words are counted.
    pub word_count_type: WordCountType,
}

impl LocaleData {
    /// Builds the data as `new WP_Locale()` does: every lookup in WordPress's order, then the word count type.
    /// `text_direction_global` is `$GLOBALS['text_direction']` when set; then the text direction is not looked up.
    pub fn build(
        strings: &impl LocaleStrings,
        text_direction_global: Option<&[u8]>,
    ) -> Result<LocaleData, PortError> {
        let values: Vec<Vec<u8>> = locale_strings(strings, text_direction_global.is_none())?
            .iter()
            .map(bytes)
            .collect();
        let weekday: Vec<Vec<u8>> = values[0..7].to_vec();
        let keyed = |keys: &[Vec<u8>], vals: &[Vec<u8>]| -> Vec<(Vec<u8>, Vec<u8>)> {
            let mut out: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
            for (k, v) in keys.iter().zip(vals) {
                // A repeated key keeps its first position and takes the last value, as a PHP array does.
                match out.iter_mut().find(|(ek, _)| ek == k) {
                    Some(slot) => slot.1 = v.clone(),
                    None => out.push((k.clone(), v.clone())),
                }
            }
            out
        };
        let numbered = |vals: &[Vec<u8>]| -> Vec<(String, Vec<u8>)> {
            vals.iter()
                .enumerate()
                .map(|(i, v)| (format!("{:02}", i + 1), v.clone()))
                .collect()
        };
        let months: Vec<Vec<u8>> = values[21..33].to_vec();
        let thousands = {
            let raw = &values[61];
            let mut replaced = Vec::with_capacity(raw.len());
            for &b in raw {
                if b == b' ' {
                    replaced.extend_from_slice(b"&nbsp;");
                } else {
                    replaced.push(b);
                }
            }
            if replaced == b"number_format_thousands_sep" {
                b",".to_vec()
            } else {
                replaced
            }
        };
        let decimal_point = if values[62] == b"number_format_decimal_point" {
            b".".to_vec()
        } else {
            values[62].clone()
        };
        let text_direction = match text_direction_global {
            Some(global) => global.to_vec(),
            None if values[64] == b"rtl" => b"rtl".to_vec(),
            None => b"ltr".to_vec(),
        };
        let word_count = strings.translate_with_context(WORD_COUNT_TYPE.0, WORD_COUNT_TYPE.1)?;
        Ok(LocaleData {
            weekday_initial: keyed(&weekday, &values[7..14]),
            weekday_abbrev: keyed(&weekday, &values[14..21]),
            month: numbered(&months),
            month_genitive: numbered(&values[33..45]),
            month_abbrev: keyed(&months, &values[45..57]),
            meridiem: ["am", "pm", "AM", "PM"]
                .into_iter()
                .zip(values[57..61].iter().cloned())
                .collect(),
            thousands_sep: thousands,
            decimal_point,
            list_item_separator: values[63].clone(),
            text_direction,
            word_count_type: WordCountType::from_translation(&bytes(&word_count)),
            weekday,
        })
    }
}

/// The value a native function would return for one lookup.
fn returned(text: &str, t: crate::Translated<'_>) -> PhpValue {
    match t {
        crate::Translated::Bytes(b) => crate::bytes_value(b),
        crate::Translated::Arg(_) => PhpValue::Str(text.to_owned()),
        crate::Translated::Value(v) => v,
    }
}

impl LocaleStrings for crate::I18nApi {
    fn translate(&self, text: &str) -> Result<PhpValue, PortError> {
        crate::I18nApi::translate(self, text.as_bytes(), None, "default", |t| {
            returned(text, t)
        })
        .map_err(|e| PortError(Box::new(e)))
    }

    fn translate_with_context(&self, text: &str, context: &str) -> Result<PhpValue, PortError> {
        crate::I18nApi::translate(
            self,
            text.as_bytes(),
            Some(context.as_bytes()),
            "default",
            |t| returned(text, t),
        )
        .map_err(|e| PortError(Box::new(e)))
    }
}
