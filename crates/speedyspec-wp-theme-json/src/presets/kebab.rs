//! `_wp_to_kebab_case()`: lodash's `kebabCase`, as WordPress ports it with one PCRE pattern.

use std::sync::LazyLock;

use fancy_regex::Regex;

/// The pattern, translated from WordPress's: PCRE's `\d` and `\b` without UCP are ASCII, and its `$` also matches
/// before a final newline, so those are spelled out.
static WORDS: LazyLock<Regex> = LazyLock::new(|| {
    let lower_range = r"a-z\x{df}-\x{f6}\x{f8}-\x{ff}";
    let non_char_range = r"\x{00}-\x{2f}\x{3a}-\x{40}\x{5b}-\x{60}\x{7b}-\x{bf}";
    let punctuation_range = r"\x{2000}-\x{206f}";
    let space_range = r" \t\x{0b}\x{0c}\x{a0}\x{feff}\n\r\x{2028}\x{2029}\x{1680}\x{180e}\x{2000}\x{2001}\x{2002}\x{2003}\x{2004}\x{2005}\x{2006}\x{2007}\x{2008}\x{2009}\x{200a}\x{202f}\x{205f}\x{3000}";
    let upper_range = r"A-Z\x{c0}-\x{d6}\x{d8}-\x{de}";
    let break_range = format!("{non_char_range}{punctuation_range}{space_range}");
    let brk = format!("[{break_range}]");
    let digits = "[0-9]+";
    let lower = format!("[{lower_range}]");
    // WordPress puts `\d+` inside this class, so `+` is excluded too.
    let misc = format!("[^{break_range}0-9+{lower_range}{upper_range}]");
    let upper = format!("[{upper_range}]");
    let misc_lower = format!("(?:{lower}|{misc})");
    let misc_upper = format!("(?:{upper}|{misc})");
    let word = "[A-Za-z0-9_]";
    let boundary = format!("(?:(?<={word})(?!{word})|(?<!{word})(?={word}))");
    let end = r"(?=\n?\z)";
    let ord_lower = format!("[0-9]*(?:1st|2nd|3rd|(?![123])[0-9]th)(?={boundary}|[A-Z_])");
    let ord_upper = format!("[0-9]*(?:1ST|2ND|3RD|(?![123])[0-9]TH)(?={boundary}|[a-z_])");
    let pattern = [
        format!("{upper}?{lower}+(?={brk}|{upper}|{end})"),
        format!("{misc_upper}+(?={brk}|{upper}{misc_lower}|{end})"),
        format!("{upper}?{misc_lower}+"),
        format!("{upper}+"),
        ord_upper,
        ord_lower,
        digits.to_owned(),
    ]
    .join("|");
    Regex::new(&pattern).expect("the kebab-case pattern compiles")
});

/// `_wp_to_kebab_case( $input_string )`. Invalid UTF-8 gives `""`, as PCRE's `/u` does.
pub fn to_kebab_case(input: &[u8]) -> String {
    let Ok(text) = std::str::from_utf8(input) else {
        return String::new();
    };
    let text = text.replace('\'', "");
    let words: Vec<&str> = WORDS
        .find_iter(&text)
        .filter_map(Result::ok)
        .map(|m| m.as_str())
        .collect();
    words.join("-").to_ascii_lowercase()
}
