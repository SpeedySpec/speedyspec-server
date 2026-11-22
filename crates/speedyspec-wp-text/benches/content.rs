//! The content filters over the corpus's whole posts and a long page, in bytes per second, comparable with the PHP
//! baseline (`php-baseline <function>`). Each function's output on the bench inputs is checked against WordPress's
//! before it is timed.
//!
//! Times are thread CPU time, not wall time, so a busy machine skews them less; compare them with the baseline's
//! CPU-time rates.

mod common;

use std::borrow::Cow;

use common::{CpuTime, api};
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use speedyspec_hooks::Value;
use speedyspec_wp_text::{convert_chars, force_balance_tags, wp_strip_all_tags_core};
use speedyspec_wp_text_corpus::{Php, cases, from_value, inputs, page};

type Bench = Criterion<CpuTime>;


fn whole_posts(function: &str) -> Vec<Vec<u8>> {
    inputs(function).into_iter().filter(|(s, _)| s.contains("/whole_posts/")).map(|(_, b)| b).collect()
}

fn bench(c: &mut Bench, function: &str, run: &dyn Fn(&[u8]) -> Cow<'_, [u8]>) {
    for case in cases(function).iter().filter(|c| c.scenario.contains("/whole_posts/")) {
        assert_eq!(Php::Str(run(case.args[0].bytes().unwrap()).into_owned()), case.returned, "{}", case.scenario);
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

fn content(c: &mut Bench) {
    let api = api();
    bench(c, "wptexturize", &|t| Cow::Owned(text(api.wptexturize(&speedyspec_wp_text::strings::bytes_value(t.to_vec()), false).unwrap())));
    bench(c, "wpautop", &|t| api.wpautop(t, true).unwrap());
    bench(c, "shortcode_unautop", &|t| api.shortcode_unautop(t).unwrap());
    bench(c, "capital_P_dangit", &|t| api.capital_p_dangit(t).unwrap());
    bench(c, "convert_chars", &convert_chars);
    bench(c, "force_balance_tags", &force_balance_tags);
    bench(c, "wp_strip_all_tags", &|t| wp_strip_all_tags_core(t, false));
    bench(c, "convert_smilies", &|t| api.convert_smilies(t).unwrap());
    bench(c, "make_clickable", &|t| Cow::Owned(api.make_clickable(t).unwrap()));
    bench(c, "wp_rel_nofollow", &|t| Cow::Owned(api.wp_rel_nofollow(t).unwrap()));

    // the_content's formatting chain, as default-filters.php orders it: wptexturize, wpautop and shortcode_unautop
    // at 10, capital_P_dangit at 11, convert_smilies at 20.
    let chain = |t: &[u8]| {
        let t = text(api.wptexturize(&speedyspec_wp_text::strings::bytes_value(t.to_vec()), false).unwrap());
        let t = api.wpautop(&t, true).unwrap().into_owned();
        let t = api.shortcode_unautop(&t).unwrap().into_owned();
        let t = api.capital_p_dangit(&t).unwrap().into_owned();
        api.convert_smilies(&t).unwrap().into_owned()
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

criterion_group! {
    name = benches;
    config = Criterion::default().with_measurement(CpuTime);
    targets = content
}
criterion_main!(benches);
