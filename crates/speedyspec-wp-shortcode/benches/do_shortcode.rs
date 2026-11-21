//! The whole `do_shortcode()` on each corpus file, with the default strategies and with the reference ones.

mod common;

use std::rc::Rc;

use criterion::{BenchmarkId, Throughput, black_box};
use speedyspec_wp_shortcode::{FancyRegexScanner, HandAttsParser, HandScanner, RegexAttsParser};

fn main() {
    let mut c = common::criterion();
    let fast = common::api(Rc::new(HandScanner::new()), Rc::new(HandAttsParser::new()));
    let reference = common::api(Rc::new(FancyRegexScanner::new()), Rc::new(RegexAttsParser::new()));
    let mut group = c.benchmark_group("do_shortcode");
    for (name, content) in common::corpus() {
        assert_eq!(fast.do_shortcode(&content, false), reference.do_shortcode(&content, false), "{name}");
        group.throughput(Throughput::Bytes(content.len() as u64));
        for (label, api) in [("hand+hand", &fast), ("fancy-regex+regex", &reference)] {
            group.bench_with_input(BenchmarkId::new(label, &name), &content, |b, content| {
                b.iter(|| api.do_shortcode(black_box(content), false).unwrap().len())
            });
        }
    }
    group.finish();
    c.final_summary();
}
