//! The shortcode scanners on each corpus file, with the tag set `do_shortcode()` would use.

mod common;

use std::hint::black_box;

use criterion::{BenchmarkId, Throughput};
use speedyspec_wp_shortcode::{FancyRegexScanner, HandScanner};
use speedyspec_wp_shortcode_contracts::ShortcodeScanner;

fn main() {
    let mut c = common::criterion();
    let hand = HandScanner::new();
    let fancy = FancyRegexScanner::new();
    let mut group = c.benchmark_group("scan");
    for (name, content) in common::corpus() {
        let tags = common::narrowed(&content);
        group.throughput(Throughput::Bytes(content.len() as u64));
        for (label, scanner) in [("hand", &hand as &dyn ShortcodeScanner), ("fancy-regex", &fancy)] {
            group.bench_with_input(BenchmarkId::new(label, &name), &content, |b, content| {
                b.iter(|| scanner.scan(black_box(content), &tags).count())
            });
        }
    }
    group.finish();
    c.final_summary();
}
