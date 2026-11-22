//! WordPress's `remove_accents()`.

use std::borrow::Cow;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::LazyLock;

use speedyspec_wp_utf8::is_valid_utf8;
use unicode_normalization::{IsNormalized, UnicodeNormalization, is_nfc_quick};

use crate::accents_table::ACCENTS;
use crate::strings::{StrReplace, StrtrTable, strtr};

/// Which of `remove_accents()`'s locale additions apply.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum LocaleGroup {
    None,
    German,
    Danish,
    Catalan,
    SerbianBosnian,
}

impl LocaleGroup {
    /// The group for a locale, as `remove_accents()` tests it.
    fn of(locale: &str) -> LocaleGroup {
        if locale.starts_with("de") {
            LocaleGroup::German
        } else if locale == "da_DK" {
            LocaleGroup::Danish
        } else if locale == "ca" {
            LocaleGroup::Catalan
        } else if locale == "sr_RS" || locale == "bs_BA" {
            LocaleGroup::SerbianBosnian
        } else {
            LocaleGroup::None
        }
    }

    /// The pairs this group adds after `$chars` (overriding a key `$chars` already has).
    fn extra(self) -> &'static [(&'static str, &'static str)] {
        match self {
            LocaleGroup::None => &[],
            LocaleGroup::German => &[
                ("Ä", "Ae"),
                ("ä", "ae"),
                ("Ö", "Oe"),
                ("ö", "oe"),
                ("Ü", "Ue"),
                ("ü", "ue"),
                ("ẞ", "SS"),
                ("ß", "ss"),
            ],
            LocaleGroup::Danish => &[("Æ", "Ae"), ("æ", "ae"), ("Ø", "Oe"), ("ø", "oe"), ("Å", "Aa"), ("å", "aa")],
            LocaleGroup::Catalan => &[("l·l", "ll")],
            LocaleGroup::SerbianBosnian => &[("Đ", "DJ"), ("đ", "dj")],
        }
    }
}

thread_local! {
    /// The `strtr()` table for each locale group, built on first use (Lazy Load).
    static TABLES: RefCell<Vec<(LocaleGroup, Rc<StrtrTable>)>> = const { RefCell::new(Vec::new()) };
}

/// The table for a locale group.
fn table(group: LocaleGroup) -> Rc<StrtrTable> {
    TABLES.with(|tables| {
        if let Some((_, table)) = tables.borrow().iter().find(|(g, _)| *g == group) {
            return table.clone();
        }
        let pairs = ACCENTS
            .iter()
            .chain(group.extra())
            .map(|(from, to)| (from.as_bytes().to_vec(), to.as_bytes().to_vec()))
            .collect();
        let table = Rc::new(StrtrTable::new(pairs));
        tables.borrow_mut().push((group, table.clone()));
        table
    })
}

/// `remove_accents()`'s single-byte map for text that isn't UTF-8 (ISO-8859-1 and Windows-1252 bytes).
static LATIN1: LazyLock<[u8; 256]> = LazyLock::new(|| {
    let from = b"\x80\x83\x8a\x8e\x9a\x9e\x9f\xa2\xa5\xb5\xc0\xc1\xc2\xc3\xc4\xc5\xc7\xc8\xc9\xca\xcb\xcc\xcd\xce\xcf\xd1\xd2\
\xd3\xd4\xd5\xd6\xd8\xd9\xda\xdb\xdc\xdd\xe0\xe1\xe2\xe3\xe4\xe5\xe7\xe8\xe9\xea\xeb\xec\xed\xee\xef\xf1\xf2\xf3\xf4\xf5\xf6\
\xf8\xf9\xfa\xfb\xfc\xfd\xff";
    let to = b"EfSZszYcYuAAAAAACEEEEIIIINOOOOOOUUUUYaaaaaaceeeeiiiinoooooouuuuyy";
    let mut map = [0u8; 256];
    for (b, slot) in map.iter_mut().enumerate() {
        *slot = b as u8;
    }
    for (&f, &t) in from.iter().zip(to.iter()) {
        map[usize::from(f)] = t;
    }
    map
});

/// `remove_accents()`'s double-character replacements for text that isn't UTF-8.
static LATIN1_DOUBLE: LazyLock<StrReplace> = LazyLock::new(|| {
    let pairs: [(&[u8], &[u8]); 9] = [
        (b"\x8c", b"OE"),
        (b"\x9c", b"oe"),
        (b"\xc6", b"AE"),
        (b"\xd0", b"DH"),
        (b"\xde", b"TH"),
        (b"\xdf", b"ss"),
        (b"\xe6", b"ae"),
        (b"\xf0", b"dh"),
        (b"\xfe", b"th"),
    ];
    StrReplace::new(pairs.iter().map(|(a, b)| (a.to_vec(), b.to_vec())).collect())
});

/// WordPress's `remove_accents()`.
///
/// @param text
///     The text.
/// @param locale
///     The locale, already resolved (WordPress asks `get_locale()` when the caller passes none).
/// @return
///     The text with accented letters replaced by unaccented ones; borrowed when it is all ASCII.
pub fn remove_accents<'a>(text: &'a [u8], locale: &str) -> Cow<'a, [u8]> {
    if text.is_ascii() {
        return Cow::Borrowed(text);
    }
    if is_valid_utf8(text) {
        let utf8 = std::str::from_utf8(text).expect("validated");
        let composed: Cow<'_, str> = match is_nfc_quick(utf8.chars()) {
            IsNormalized::Yes => Cow::Borrowed(utf8),
            _ if unicode_normalization::is_nfc(utf8) => Cow::Borrowed(utf8),
            _ => Cow::Owned(utf8.nfc().collect()),
        };
        let replaced = strtr(composed.as_bytes(), &table(LocaleGroup::of(locale))).into_owned();
        return Cow::Owned(replaced);
    }
    let mapped: Vec<u8> = text.iter().map(|&b| LATIN1[usize::from(b)]).collect();
    Cow::Owned(LATIN1_DOUBLE.apply(&mapped).into_owned())
}
