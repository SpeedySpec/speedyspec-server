//! Every content filter with a facade method, through `TextApi`, against the corpus, diagnostics included.
//!
//! Functions with no filter, setting or report (`normalize_whitespace`, `convert_invalid_entities`,
//! `wp_replace_in_html_tags`, `strip_tags`) have no facade method; their own tests check them.

mod common;

use speedyspec_hooks::Value;
use speedyspec_wp_text_corpus::{Php, assert_corpus_reporting, diagnostics, from_value, to_value};
use speedyspec_wp_text_contracts::WordCountType;
use speedyspec_wp_text_memory::{FixedShortcodeTags, FixedSiteSettings};

#[path = "common/api.rs"]
mod api;

fn check(function: &str, run: impl Fn(&api::Harness, &[Php]) -> Php) {
    let h = api::harness();
    assert_corpus_reporting(function, |args| {
        let out = run(&h, args);
        (out, diagnostics(&h.diagnostics.taken()))
    });
}

fn s(bytes: Vec<u8>) -> Php {
    Php::Str(bytes)
}

fn text(args: &[Php]) -> &[u8] {
    common::text(&args[0])
}

#[test]
fn wptexturize() {
    check("wptexturize", |h, args| {
        let reset = args.get(1).is_some_and(|r| !common::php_empty(r));
        from_value(&h.api.wptexturize(&to_value(&args[0]), reset).unwrap())
    });
}

#[test]
fn wpautop() {
    check("wpautop", |h, args| {
        let br = args.get(1).is_none_or(|b| !common::php_empty(b));
        s(h.api.wpautop(text(args), br).unwrap().into_owned())
    });
}

#[test]
fn shortcode_unautop() {
    check("shortcode_unautop", |h, args| s(h.api.shortcode_unautop(text(args)).unwrap().into_owned()));
}

#[test]
fn capital_p_dangit() {
    check("capital_P_dangit", |h, args| s(h.api.capital_p_dangit(text(args)).unwrap().into_owned()));
}

#[test]
fn convert_chars() {
    check("convert_chars", |h, args| {
        s(h.api.convert_chars(text(args), &args.get(1).map_or(Value::Str(String::new()), to_value)).unwrap().into_owned())
    });
}

#[test]
fn balance_tags() {
    check("balanceTags", |h, args| {
        s(h.api.balance_tags(text(args), !common::php_empty(&args[1])).unwrap().into_owned())
    });
    check("force_balance_tags", |h, args| s(h.api.balance_tags(text(args), true).unwrap().into_owned()));
}

#[test]
fn ent2ncr() {
    check("ent2ncr", |h, args| from_value(&h.api.ent2ncr(text(args)).unwrap()));
}

#[test]
fn wp_strip_all_tags() {
    check("wp_strip_all_tags", |h, args| {
        let remove_breaks = args.get(1).is_some_and(|b| !common::php_empty(b));
        s(h.api.wp_strip_all_tags(&to_value(&args[0]), remove_breaks).unwrap())
    });
}

fn trim(h: &api::Harness, args: &[Php]) -> Php {
    let more = match args.get(2) {
        None | Some(Php::Null) => None,
        Some(more) => Some(common::text(more)),
    };
    from_value(&h.api.wp_trim_words(text(args), args.get(1).map_or(55, common::php_int), more).unwrap())
}

#[test]
fn wp_trim_words() {
    check("wp_trim_words", trim);
}

#[test]
fn wp_trim_words_by_characters() {
    let settings = FixedSiteSettings {
        word_count_type: WordCountType::CharactersIncludingSpaces,
        ..FixedSiteSettings::default()
    };
    let h = api::harness_with(settings, std::rc::Rc::new(FixedShortcodeTags(api::tags())));
    assert_corpus_reporting("speedyspec_wp_trim_words_characters", |args| {
        (trim(&h, args), diagnostics(&h.diagnostics.taken()))
    });
}

#[test]
fn wp_html_excerpt() {
    check("wp_html_excerpt", |h, args| {
        let more = args.get(2).map(common::text);
        s(h.api.wp_html_excerpt(text(args), common::php_int(&args[1]), more).unwrap())
    });
}

#[test]
fn format_for_editor() {
    check("format_for_editor", |h, args| {
        from_value(&h.api.format_for_editor(text(args), &Value::Null).unwrap())
    });
}

/// wpTexturize.php's filter_translate() (table 1) and filter_translate2() (table 2), as a translator.
struct TestTranslations(i64);

impl speedyspec_wp_text_contracts::Translator for TestTranslations {
    fn translate(&self, text: &str, _: &str) -> Result<String, speedyspec_wp_text_contracts::TextError> {
        Ok(text.to_string())
    }

    fn translate_with_context(
        &self,
        text: &str,
        context: &str,
        _: &str,
    ) -> Result<String, speedyspec_wp_text_contracts::TextError> {
        let one = self.0 == 1;
        Ok(match text {
            "&#8211;" => "!endash!",
            "&#8212;" => "!emdash!",
            "&#8216;" if one => "!openq1!",
            "&#8216;" => "!q1!",
            "&#8217;" if context == "apostrophe" => "!apos!",
            "&#8217;" if one => "!closeq1!",
            "&#8217;" => "!q1!",
            "&#8220;" if one => "!openq2!",
            "&#8221;" if one => "!closeq2!",
            "&#8220;" | "&#8221;" => "!q2!",
            "&#8242;" => "!prime1!",
            "&#8243;" => "!prime2!",
            "&#8217;tain&#8217;t,&#8217;twere,&#8217;twas,&#8217;tis,&#8217;twill,&#8217;til,&#8217;bout,&#8217;nuff,&#8217;round,&#8217;cause,&#8217;em" if one => {
                "!apos!tain!apos!t,!apos!twere,!apos!twas,!apos!tis,!apos!twill,!apos!til,!apos!bout,!apos!nuff,!apos!round,!apos!cause,!apos!em"
            }
            other => other,
        }
        .to_string())
    }
}

#[test]
fn translated_wptexturize() {
    let harnesses: Vec<api::Harness> = [1, 2]
        .into_iter()
        .map(|table| {
            api::harness_full(
                FixedSiteSettings::default(),
                std::rc::Rc::new(FixedShortcodeTags(api::tags())),
                std::rc::Rc::new(TestTranslations(table)),
            )
        })
        .collect();
    assert_corpus_reporting("speedyspec_wptexturize_translated", |args| {
        let h = &harnesses[if common::php_int(&args[1]) == 1 { 0 } else { 1 }];
        let out = from_value(&h.api.wptexturize(&to_value(&args[0]), true).unwrap());
        (out, diagnostics(&h.diagnostics.taken()))
    });
}

#[test]
fn wptexturize_with_no_texturize_shortcodes_filter() {
    use speedyspec_wp_hooks_contracts::{Callback, HookName, HookValue, Key};
    let h = api::harness();
    h.hooks
        .add_filter(
            &HookName::from("no_texturize_shortcodes"),
            Callback::rust("filter_shortcodes", |args: &[HookValue]| {
                let HookValue::Array(mut list) = args[0].clone() else { panic!("a list") };
                list.push((Key::Int(list.len() as i64), HookValue::Str("audio".into())));
                Ok(HookValue::Array(list))
            }),
            &HookValue::Int(10),
            1,
        )
        .unwrap();
    assert_corpus_reporting("speedyspec_wptexturize_no_texturize_shortcodes", |args| {
        let out = from_value(&h.api.wptexturize(&to_value(&args[0]), false).unwrap());
        (out, diagnostics(&h.diagnostics.taken()))
    });
}

#[test]
fn capital_p_dangit_inside_a_title_filter() {
    use speedyspec_wp_hooks_contracts::{Callback, HookName, HookValue};
    let h = api::harness();
    for hook in ["the_title", "wp_title"] {
        let api = h.api.clone();
        h.hooks
            .add_filter(
                &HookName::from(hook),
                Callback::rust("capital_P_dangit", move |args: &[HookValue]| {
                    let HookValue::Str(text) = &args[0] else { panic!("a string") };
                    Ok(HookValue::Bytes(api.capital_p_dangit(text.as_bytes()).unwrap().into_owned()))
                }),
                &HookValue::Int(11),
                1,
            )
            .unwrap();
    }
    assert_corpus_reporting("speedyspec_capital_P_dangit_in_filter", |args| {
        let hook = String::from_utf8(common::text(&args[1]).to_vec()).unwrap();
        let out = h.hooks.apply_filters(&HookName::from(hook.as_str()), to_value(&args[0]), Vec::new()).unwrap();
        (from_value(&out), diagnostics(&h.diagnostics.taken()))
    });
}
