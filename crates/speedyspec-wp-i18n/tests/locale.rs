//! WP_Locale's strings: WordPress's lookups in WordPress's order, and the values `WP_Locale::init()` derives.

use std::cell::RefCell;

use speedyspec_wp_i18n::{LOCALE_STRINGS, LocaleData, locale_strings};
use speedyspec_wp_i18n_contracts::{LocaleStrings, PhpValue, PortError, WordCountType};

/// Records each lookup and answers from a table (identity when missing).
#[derive(Default)]
struct Recording {
    calls: RefCell<Vec<(String, Option<String>)>>,
    answers: Vec<((&'static str, Option<&'static str>), &'static str)>,
}

impl Recording {
    fn answering(answers: Vec<((&'static str, Option<&'static str>), &'static str)>) -> Recording {
        Recording {
            answers,
            ..Recording::default()
        }
    }

    fn answer(&self, text: &str, context: Option<&str>) -> PhpValue {
        self.calls
            .borrow_mut()
            .push((text.to_owned(), context.map(str::to_owned)));
        let found = self
            .answers
            .iter()
            .find(|((t, c), _)| *t == text && *c == context)
            .map(|(_, a)| *a)
            .unwrap_or(text);
        PhpValue::Str(found.to_owned())
    }
}

impl LocaleStrings for Recording {
    fn translate(&self, text: &str) -> Result<PhpValue, PortError> {
        Ok(self.answer(text, None))
    }

    fn translate_with_context(&self, text: &str, context: &str) -> Result<PhpValue, PortError> {
        Ok(self.answer(text, Some(context)))
    }
}

/// WordPress 7.1.5's `WP_Locale::init()` lookups, in order, before the text direction.
const WORDPRESS: &[(&str, Option<&str>)] = &[
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

fn owned(list: &[(&str, Option<&str>)]) -> Vec<(String, Option<String>)> {
    list.iter()
        .map(|(t, c)| ((*t).to_owned(), c.map(str::to_owned)))
        .collect()
}

#[test]
fn the_table_is_wordpress_order() {
    assert_eq!(owned(LOCALE_STRINGS), owned(WORDPRESS));
}

#[test]
fn strings_are_looked_up_in_wordpress_order() {
    let r = Recording::default();
    let values = locale_strings(&r, true).unwrap();
    let mut want = owned(WORDPRESS);
    want.push(("ltr".into(), Some("text direction".into())));
    assert_eq!(*r.calls.borrow(), want);
    assert_eq!(values.len(), WORDPRESS.len() + 1);
    assert_eq!(values[0], PhpValue::Str("Sunday".into()));
}

#[test]
fn text_direction_is_not_asked_when_the_global_is_set() {
    let r = Recording::default();
    let values = locale_strings(&r, false).unwrap();
    assert_eq!(values.len(), WORDPRESS.len());
    assert_eq!(r.calls.borrow().len(), WORDPRESS.len());
}

#[test]
fn build_calls_in_wordpress_order_then_word_count() {
    let r = Recording::default();
    LocaleData::build(&r, None).unwrap();
    let mut want = owned(WORDPRESS);
    want.push(("ltr".into(), Some("text direction".into())));
    want.push((
        "words".into(),
        Some("Word count type. Do not translate!".into()),
    ));
    assert_eq!(*r.calls.borrow(), want);
}

#[test]
fn untranslated_number_format_falls_back() {
    let d = LocaleData::build(&Recording::default(), None).unwrap();
    assert_eq!(d.thousands_sep, b",");
    assert_eq!(d.decimal_point, b".");
    assert_eq!(d.list_item_separator, b", ");
    assert_eq!(d.month[0], ("01".to_owned(), b"January".to_vec()));
    assert_eq!(d.weekday_initial[0], (b"Sunday".to_vec(), b"S".to_vec()));
}

#[test]
fn thousands_sep_space_becomes_nbsp() {
    let r = Recording::answering(vec![(("number_format_thousands_sep", None), " ")]);
    assert_eq!(
        LocaleData::build(&r, None).unwrap().thousands_sep,
        b"&nbsp;"
    );
}

#[test]
fn rtl_only_when_translated_rtl() {
    let rtl = Recording::answering(vec![(("ltr", Some("text direction")), "rtl")]);
    assert_eq!(
        LocaleData::build(&rtl, None).unwrap().text_direction,
        b"rtl"
    );
    assert_eq!(
        LocaleData::build(&Recording::default(), None)
            .unwrap()
            .text_direction,
        b"ltr"
    );
    let other = Recording::answering(vec![(("ltr", Some("text direction")), "ttb")]);
    assert_eq!(
        LocaleData::build(&other, None).unwrap().text_direction,
        b"ltr"
    );
}

#[test]
fn the_text_direction_global_wins_without_asking() {
    let r = Recording::answering(vec![(("ltr", Some("text direction")), "rtl")]);
    let d = LocaleData::build(&r, Some(b"ltr")).unwrap();
    assert_eq!(d.text_direction, b"ltr");
    assert!(!r.calls.borrow().iter().any(|(t, _)| t == "ltr"));
}

#[test]
fn word_count_type_normalised() {
    for (answer, want) in [
        (
            "characters_excluding_spaces",
            WordCountType::CharactersExcludingSpaces,
        ),
        (
            "characters_including_spaces",
            WordCountType::CharactersIncludingSpaces,
        ),
        ("words", WordCountType::Words),
        ("letters", WordCountType::Words),
    ] {
        let r = Recording::answering(vec![(
            ("words", Some("Word count type. Do not translate!")),
            answer,
        )]);
        assert_eq!(LocaleData::build(&r, None).unwrap().word_count_type, want);
    }
    assert_eq!(
        WordCountType::from_translation(b"characters_including_spaces").as_str(),
        "characters_including_spaces"
    );
}
