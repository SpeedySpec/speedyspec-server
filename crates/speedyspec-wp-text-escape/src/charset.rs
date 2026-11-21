//! Character sets, as PHP's `htmlspecialchars()` and WordPress's `_canonical_charset()` name them.

use std::borrow::Cow;

/// A character set `htmlspecialchars()` knows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Charset {
    /// UTF-8, which PHP also assumes for any name it does not know.
    Utf8,
    /// A single-byte set: only ASCII's special characters matter, and no byte is invalid.
    SingleByte(SingleByte),
    /// An East Asian multibyte set (Big5, GB2312, Shift_JIS, EUC-JP). PHP validates their sequences and returns an
    /// empty string for an invalid one; the port treats them as single-byte. This is a documented break
    /// (`compat/formatting/breaks.json`), and the output stays safe, because no special character can be a trail byte.
    Unsupported(String),
}

/// The single-byte sets in PHP's table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SingleByte {
    Iso8859_1,
    Iso8859_5,
    Iso8859_15,
    Cp866,
    Cp1251,
    Cp1252,
    Koi8R,
    MacRoman,
}

impl Charset {
    /// The set PHP's `htmlspecialchars()` picks for `name`: its charset table, matched ignoring case, and UTF-8 for
    /// an empty or unknown name.
    pub fn from_php_name(name: &str) -> Charset {
        use SingleByte::*;
        const TABLE: &[(&str, Option<SingleByte>)] = &[
            ("ISO-8859-1", Some(Iso8859_1)),
            ("ISO8859-1", Some(Iso8859_1)),
            ("ISO-8859-15", Some(Iso8859_15)),
            ("ISO8859-15", Some(Iso8859_15)),
            ("cp1252", Some(Cp1252)),
            ("Windows-1252", Some(Cp1252)),
            ("1252", Some(Cp1252)),
            ("KOI8-R", Some(Koi8R)),
            ("koi8-ru", Some(Koi8R)),
            ("koi8r", Some(Koi8R)),
            ("cp1251", Some(Cp1251)),
            ("Windows-1251", Some(Cp1251)),
            ("win-1251", Some(Cp1251)),
            ("iso8859-5", Some(Iso8859_5)),
            ("iso-8859-5", Some(Iso8859_5)),
            ("cp866", Some(Cp866)),
            ("866", Some(Cp866)),
            ("ibm866", Some(Cp866)),
            ("MacRoman", Some(MacRoman)),
            ("BIG5", None),
            ("950", None),
            ("GB2312", None),
            ("936", None),
            ("BIG5-HKSCS", None),
            ("Shift_JIS", None),
            ("SJIS", None),
            ("932", None),
            ("SJIS-win", None),
            ("CP932", None),
            ("EUCJP", None),
            ("EUC-JP", None),
            ("eucJP-win", None),
        ];
        TABLE
            .iter()
            .find(|(known, _)| known.eq_ignore_ascii_case(name))
            .map_or(Charset::Utf8, |(_, set)| match set {
                Some(set) => Charset::SingleByte(*set),
                None => Charset::Unsupported(name.to_string()),
            })
    }
}

/// WordPress's `_canonical_charset()`: `UTF-8` for any spelling of UTF-8 WordPress knows, `ISO-8859-1` for its two
/// spellings, and the name unchanged otherwise.
pub fn canonical_charset(name: &str) -> Cow<'_, str> {
    if name.eq_ignore_ascii_case("utf-8") || name.eq_ignore_ascii_case("utf8") {
        Cow::Borrowed("UTF-8")
    } else if name.eq_ignore_ascii_case("iso-8859-1") || name.eq_ignore_ascii_case("iso8859-1") {
        Cow::Borrowed("ISO-8859-1")
    } else {
        Cow::Borrowed(name)
    }
}
