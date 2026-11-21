//! The content filters over the corpus's whole posts and a long page, in bytes per second, comparable with the PHP
//! baseline (`php-baseline <function>`). Each function's output on the bench inputs is checked against WordPress's
//! before it is timed.

use std::rc::Rc;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use speedyspec_hooks::Value;
use speedyspec_wp_hooks::{HookApi, IgnoreDeprecations};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_shortcode_contracts::TagName;
use speedyspec_wp_text::{TextApi, TextDeps, convert_chars, force_balance_tags, wp_strip_all_tags_core};
use speedyspec_wp_text_corpus::{Php, cases, from_value, inputs, page};
use speedyspec_wp_text_memory::{
    EnglishTranslator, FixedShortcodeTags, FixedSiteSettings, FixedTexturizeOverrides, RecordingDiagnostics,
};

fn api() -> TextApi {
    let tags = ["wp_caption", "caption", "gallery", "playlist", "audio", "video", "embed"]
        .iter()
        .map(|t| TagName::new(t.as_bytes()).unwrap())
        .collect();
    TextApi::new(TextDeps {
        hooks: Rc::new(HookApi::new(Rc::new(MemoryHookStore::new()), Rc::new(IgnoreDeprecations))),
        settings: Rc::new(FixedSiteSettings::default()),
        translator: Rc::new(EnglishTranslator),
        diagnostics: Rc::new(RecordingDiagnostics::default()),
        texturize_overrides: Rc::new(FixedTexturizeOverrides::default()),
        shortcode_tags: Rc::new(FixedShortcodeTags(tags)),
    })
}

fn whole_posts(function: &str) -> Vec<Vec<u8>> {
    inputs(function).into_iter().filter(|(s, _)| s.contains("/whole_posts/")).map(|(_, b)| b).collect()
}

fn bench(c: &mut Criterion, function: &str, run: &dyn Fn(&[u8]) -> Vec<u8>) {
    for case in cases(function).iter().filter(|c| c.scenario.contains("/whole_posts/")) {
        assert_eq!(Php::Str(run(case.args[0].bytes().unwrap())), case.returned, "{}", case.scenario);
    }
    let posts = whole_posts(function);
    let mut group = c.benchmark_group(function);
    group.throughput(Throughput::Bytes(posts.iter().map(Vec::len).sum::<usize>() as u64));
    group.bench_function("whole_posts", |b| {
        b.iter(|| {
            for post in &posts {
                std::hint::black_box(run(std::hint::black_box(post)));
            }
        })
    });
    let page = page(function);
    group.throughput(Throughput::Bytes(page.len() as u64));
    group.bench_function("page", |b| b.iter(|| std::hint::black_box(run(std::hint::black_box(&page)))));
    group.finish();
}

fn text(value: Value) -> Vec<u8> {
    match from_value(&value) {
        Php::Str(s) => s,
        other => panic!("not a string: {other}"),
    }
}

fn content(c: &mut Criterion) {
    let api = api();
    bench(c, "wptexturize", &|t| text(api.wptexturize(&speedyspec_wp_text::strings::bytes_value(t.to_vec()), false).unwrap()));
    bench(c, "wpautop", &|t| api.wpautop(t, true).unwrap());
    bench(c, "shortcode_unautop", &|t| api.shortcode_unautop(t).unwrap());
    bench(c, "capital_P_dangit", &|t| api.capital_p_dangit(t).unwrap());
    bench(c, "convert_chars", &|t| convert_chars(t).into_owned());
    bench(c, "force_balance_tags", &|t| force_balance_tags(t).into_owned());
    bench(c, "wp_strip_all_tags", &|t| wp_strip_all_tags_core(t, false).into_owned());

    // the_content's formatting chain, as default-filters.php orders it (convert_smilies waits for plan 4).
    let chain = |t: &[u8]| {
        let t = text(api.wptexturize(&speedyspec_wp_text::strings::bytes_value(t.to_vec()), false).unwrap());
        let t = api.wpautop(&t, true).unwrap();
        let t = api.shortcode_unautop(&t).unwrap();
        api.capital_p_dangit(&t).unwrap()
    };
    let posts = whole_posts("wptexturize");
    let mut group = c.benchmark_group("the_content_chain");
    group.throughput(Throughput::Bytes(posts.iter().map(Vec::len).sum::<usize>() as u64));
    group.bench_function("whole_posts", |b| {
        b.iter(|| {
            for post in &posts {
                std::hint::black_box(chain(std::hint::black_box(post)));
            }
        })
    });
    let page = page("wptexturize");
    group.throughput(Throughput::Bytes(page.len() as u64));
    group.bench_function("page", |b| b.iter(|| std::hint::black_box(chain(std::hint::black_box(&page)))));
    group.finish();
}

criterion_group!(benches, content);
criterion_main!(benches);
