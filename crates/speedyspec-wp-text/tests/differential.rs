//! The hand-written fast paths against the PCRE2 transcriptions of WordPress's code.

mod common;

use proptest::prelude::*;
use speedyspec_wp_shortcode_contracts::TagName;
use speedyspec_wp_text::{
    AutopOptions, DEFAULT_SPACES, HandTexturizer, PcreTexturizer, TexturizeScope, TexturizeTables, Texturizer, wpautop,
    wpautop_reference,
};
use speedyspec_wp_text_corpus::{cases, inputs};

fn tags() -> Vec<TagName> {
    ["wp_caption", "caption", "gallery", "playlist", "audio", "video", "embed"]
        .iter()
        .map(|t| TagName::new(t.as_bytes()).unwrap())
        .collect()
}

fn english() -> TexturizeTables {
    TexturizeTables::build(&|text: &str, _: &str| text.to_string(), None, DEFAULT_SPACES).unwrap()
}

fn list(items: &[&str]) -> Vec<Vec<u8>> {
    items.iter().map(|s| s.as_bytes().to_vec()).collect()
}

fn texturize_both(tables: &TexturizeTables, text: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let no_tags = list(&["pre", "code", "kbd", "style", "script", "tt"]);
    let no_shortcodes = list(&["code"]);
    let tags = tags();
    let scope = TexturizeScope { no_texturize_tags: &no_tags, no_texturize_shortcodes: &no_shortcodes, shortcode_tags: &tags };
    let reference = PcreTexturizer::default().texturize(text, tables, &scope).unwrap().into_owned();
    let hand = HandTexturizer::default().texturize(text, tables, &scope).unwrap().into_owned();
    (reference, hand)
}

const TEXTURIZE_TOKENS: &[&str] = &[
    "'", "\"", "-", "--", "---", ".", "...", "x", "0", "9", "12", "a", "xn", "<", ">", "[", "]", "/", "!", " ", "\n",
    "\t", "\u{a0}", "&nbsp;", "&lt;", "&gt;", "&amp;", "&", "&#38;", "&#x26;", ";", "%", ",", "(", ")", "``", "''",
    " (tm)", "'tis", "<code>", "</code>", "<!--", "-->", "[gallery]", "[/gallery]", "[code]", "[/code]", "[[gallery]]",
    "<b title=\"'\">", "<![CDATA[", "]]>", "\\", "$1",
];

const AUTOP_TOKENS: &[&str] = &[
    "<", ">", "/", "p", "pre", "div", "br", "<br />", "<br>", "<br/>", "\n", "\n\n", "\r", "\r\n", " ", "\t", "a",
    "<option>", "</option>", "[", "]", "<p>", "</p>", "<div>", "</div>", "<pre>", "</pre>", "<li>", "<ul>", "</ul>",
    "<blockquote>", "</blockquote>", "<table>", "<td>", "<hr>", "<hr />", "<h2>", "</h2>", "<script>", "</script>",
    "<object>", "</object>", "<embed>", "<source>", "<video>", "</video>", "<figcaption>", "</figcaption>", "<!--",
    "-->", "<math>", "</math>", "<svg>", "</svg>", "<section ", "</section>", "<address>", "</address>", "<form>",
    "</form>", "\u{a0}", "x", "<span>", "<em>", "<img />", "<a href=\"x\">", "<thx>", "<pa>", "<hx>", "<dz>",
    // Every $allblocks name, open and closed.
    "<table>", "</table>", "<thead>", "</thead>", "<tfoot>", "</tfoot>", "<caption>", "</caption>", "<col>", "</col>", "<colgroup>", "</colgroup>", "<tbody>", "</tbody>", "<tr>", "</tr>", "<td>", "</td>", "<th>", "</th>", "<div>", "</div>", "<dl>", "</dl>", "<dd>", "</dd>", "<dt>", "</dt>", "<ul>", "</ul>", "<ol>", "</ol>", "<li>", "</li>", "<pre>", "</pre>", "<form>", "</form>", "<map>", "</map>", "<area>", "</area>", "<blockquote>", "</blockquote>", "<address>", "</address>", "<style>", "</style>", "<p>", "</p>", "<h1>", "</h1>", "<h2>", "</h2>", "<h3>", "</h3>", "<h4>", "</h4>", "<h5>", "</h5>", "<h6>", "</h6>", "<hr>", "</hr>", "<fieldset>", "</fieldset>", "<legend>", "</legend>", "<section>", "</section>", "<article>", "</article>", "<aside>", "</aside>", "<hgroup>", "</hgroup>", "<header>", "</header>", "<footer>", "</footer>", "<nav>", "</nav>", "<figure>", "</figure>", "<figcaption>", "</figcaption>", "<details>", "</details>", "<menu>", "</menu>", "<summary>", "</summary>",
];

fn joined(tokens: &[&str]) -> Vec<u8> {
    tokens.concat().into_bytes()
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 4096, ..ProptestConfig::default() })]

    #[test]
    fn hand_texturizer_matches_reference(tokens in prop::collection::vec(prop::sample::select(TEXTURIZE_TOKENS), 0..24)) {
        let text = joined(&tokens);
        let (reference, hand) = texturize_both(&english(), &text);
        prop_assert_eq!(String::from_utf8_lossy(&hand), String::from_utf8_lossy(&reference), "input {:?}", String::from_utf8_lossy(&text));
    }

    #[test]
    fn wpautop_matches_reference(tokens in prop::collection::vec(prop::sample::select(AUTOP_TOKENS), 0..32), br in any::<bool>()) {
        let text = joined(&tokens);
        let options = AutopOptions { br };
        let reference = wpautop_reference(&text, options).into_owned();
        let fast = wpautop(&text, options).into_owned();
        prop_assert_eq!(String::from_utf8_lossy(&fast), String::from_utf8_lossy(&reference), "input {:?}", String::from_utf8_lossy(&text));
    }
}

#[test]
fn hand_texturizer_matches_reference_on_corpus() {
    let tables = english();
    let mut texts: Vec<Vec<u8>> = cases("wptexturize").into_iter().filter_map(|c| c.args[0].bytes().map(<[u8]>::to_vec)).collect();
    texts.extend(inputs("wpautop").into_iter().map(|(_, b)| b));
    for text in texts {
        let (reference, hand) = texturize_both(&tables, &text);
        assert_eq!(String::from_utf8_lossy(&hand), String::from_utf8_lossy(&reference), "input {:?}", String::from_utf8_lossy(&text));
    }
}

#[test]
fn wpautop_matches_reference_on_corpus() {
    let mut texts: Vec<Vec<u8>> = inputs("wpautop").into_iter().map(|(_, b)| b).collect();
    texts.extend(inputs("wptexturize").into_iter().map(|(_, b)| b));
    for text in texts {
        for br in [true, false] {
            let options = AutopOptions { br };
            assert_eq!(
                String::from_utf8_lossy(&wpautop(&text, options)),
                String::from_utf8_lossy(&wpautop_reference(&text, options)),
                "input {:?}",
                String::from_utf8_lossy(&text)
            );
        }
    }
}

#[test]
fn texturize_falls_back_on_custom_spaces() {
    let tables = TexturizeTables::build(&|text: &str, _: &str| text.to_string(), None, b"[\\r\\n\\t ]|_").unwrap();
    assert!(!tables.hand_compatible());
    let (reference, hand) = texturize_both(&tables, b"_'a'_ x--y");
    assert_eq!(hand, reference);
}

#[test]
fn translated_tables_use_the_hand_path() {
    let tables = TexturizeTables::build(&|text: &str, context: &str| format!("[{context}:{text}]"), None, DEFAULT_SPACES).unwrap();
    assert!(tables.hand_compatible());
    for text in ["'99' \"a\" don't 9' 9\" -- --- 'tis", "<p>'a' [gallery]\"b\"[/gallery]</p>"] {
        let (reference, hand) = texturize_both(&tables, text.as_bytes());
        assert_eq!(String::from_utf8_lossy(&hand), String::from_utf8_lossy(&reference));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4096))]

    #[test]
    fn slug_matches_reference(input in proptest::collection::vec(proptest::sample::select(vec![
        &b"a"[..], b"Z", b"0", b"-", b"--", b"---", b".", b" ", b"\t", b"\n", b"&", b";", b"&amp;", b"&nbsp;", b"%", b"%2f", b"%C3%A9",
        b"_", b"/", b"<b>", b"\xC3\xA9", b"\xE2\x80\x93", b"\xC2\xA0", b"\xFF", b"\xC3\x97", b"\xE2\x80\x8B",
    ]), 0..90).prop_map(|p| p.concat()), save in any::<bool>()) {
        use speedyspec_wp_text::title::{SanitizeContext, sanitize_title_with_dashes, sanitize_title_with_dashes_reference};
        let context = if save { SanitizeContext::Save } else { SanitizeContext::Display };
        prop_assert_eq!(sanitize_title_with_dashes(&input, &context), sanitize_title_with_dashes_reference(&input, &context));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4096))]

    #[test]
    fn clickable_tag_checks_match_patterns(input in proptest::collection::vec(proptest::sample::select(vec![
        &b"<"[..], b"code", b"CODE", b"pre", b"script", b"style", b" ", b"\t", b"\x0B", b">", b"http", b"://", b"a", b"_", b"1",
        b"aaaaaaaaaaaaaaaaaaaa", b":", b"/", b"www.", b"FTP.", b"@", b".",
    ]), 1..10).prop_map(|p| [&b"<"[..], &p.concat()].concat())) {
        use speedyspec_wp_text::links::{may_link, opens, tag_starts_with_url};
        use speedyspec_wp_text::preg::{Pattern, preg_match};
        let found = |source: &str| matches!(preg_match(&Pattern::php(source), &input), Ok(Some(_)));
        for name in ["code", "pre", "script", "style"] {
            prop_assert_eq!(opens(&input, name.as_bytes()), found(&format!("|^<{name}[\\s>]|i")));
        }
        prop_assert_eq!(tag_starts_with_url(&input), found("|^<\\s*[\\w]{1,20}+://|"));
        let any = found("~[\\w]{1,20}+://~") || found("#(www|ftp)\\.#i") || found("/@/");
        prop_assert!(may_link(&input) || !any);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4096))]

    #[test]
    fn convert_chars_matches_pattern(input in proptest::collection::vec(proptest::sample::select(vec![
        &b"&"[..], b"#", b";", b"a", b"Z", b"1", b"4", b"5", b"x", b" ", b"\n", b"amp", b"abcdefgh", b"abcdefghi", b"&&",
    ]), 0..14).prop_map(|p| p.concat())) {
        use speedyspec_wp_text::preg::{Pattern, preg_replace};
        let pattern = Pattern::php("/&([^#])(?![a-z1-4]{1,8};)/i");
        let expected = preg_replace(&pattern, &input, b"&#038;$1", None).unwrap().0.into_owned();
        prop_assert_eq!(speedyspec_wp_text::convert_chars(&input).into_owned(), expected);
    }

    #[test]
    fn strip_all_tags_prefilter_is_exact(input in proptest::collection::vec(proptest::sample::select(vec![
        &b"<"[..], b">", b"/", b"script", b"SCRIPT", b"style", b"Style", b"x", b" ", b"\n", b"<br />",
    ]), 0..14).prop_map(|p| p.concat()), breaks in any::<bool>()) {
        use speedyspec_wp_text::preg::{Pattern, preg_replace};
        let pattern = Pattern::php("@<(script|style)[^>]*?>.*?</\\1>@si");
        let without = preg_replace(&pattern, &input, b"", None).unwrap().0.into_owned();
        let mut expected = speedyspec_wp_text::strip_tags(&without).into_owned();
        if breaks {
            expected = preg_replace(&Pattern::php("/[\\r\\n\\t ]+/"), &expected, b" ", None).unwrap().0.into_owned();
        }
        let expected = speedyspec_wp_text::strings::php_trim(&expected, speedyspec_wp_text::strings::PHP_TRIM_DEFAULT).to_vec();
        prop_assert_eq!(speedyspec_wp_text::wp_strip_all_tags_core(&input, breaks).into_owned(), expected);
    }
}
