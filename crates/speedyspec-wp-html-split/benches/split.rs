//! `wp_html_split` over the corpus's whole posts and a long page, in bytes per second, comparable with the PHP
//! baseline (`php-baseline wp_html_split`).

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use speedyspec_wp_html_split::wp_html_split;
use speedyspec_wp_text_corpus::{Php, assert_corpus, inputs, page};

fn split(c: &mut Criterion) {
    // The output timed is the output WordPress gives.
    assert_corpus("wp_html_split", |args| {
        Php::list(wp_html_split(args[0].bytes().unwrap()))
    });

    let posts: Vec<Vec<u8>> = inputs("wp_html_split")
        .into_iter()
        .filter(|(scenario, _)| scenario.contains("/whole_posts/"))
        .map(|(_, bytes)| bytes)
        .collect();
    let total: usize = posts.iter().map(Vec::len).sum();
    let mut group = c.benchmark_group("wp_html_split");
    group.throughput(Throughput::Bytes(total as u64));
    group.bench_function("whole_posts", |b| {
        b.iter(|| {
            for post in &posts {
                std::hint::black_box(wp_html_split(std::hint::black_box(post)));
            }
        })
    });
    let page = page("wp_html_split");
    group.throughput(Throughput::Bytes(page.len() as u64));
    group.bench_function("page", |b| {
        b.iter(|| std::hint::black_box(wp_html_split(std::hint::black_box(&page))))
    });
    group.finish();
}

criterion_group!(benches, split);
criterion_main!(benches);
