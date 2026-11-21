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
