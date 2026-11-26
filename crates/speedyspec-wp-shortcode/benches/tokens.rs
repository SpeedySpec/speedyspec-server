//! `do_shortcode()`'s tag prefilter: the `memchr` token scan against the `regex` crate on WordPress's pattern.

mod common;

use std::hint::black_box;

use criterion::{BenchmarkId, Throughput};
use speedyspec_wp_shortcode::candidate_tokens;

fn main() {
    let mut c = common::criterion();
    let re = regex::bytes::Regex::new(r"(?-u)\[([^<>&/\[\]\x00-\x20=]+)").unwrap();
    let mut group = c.benchmark_group("tokens");
    for (name, content) in common::corpus() {
        group.throughput(Throughput::Bytes(content.len() as u64));
        group.bench_with_input(BenchmarkId::new("memchr", &name), &content, |b, content| {
            b.iter(|| candidate_tokens(black_box(content)).count())
        });
        group.bench_with_input(BenchmarkId::new("regex", &name), &content, |b, content| {
            b.iter(|| re.captures_iter(black_box(content)).count())
        });
    }
    group.finish();
    c.final_summary();
}
