//! The attribute parsers on every attribute text the corpus's shortcodes have.

mod common;

use std::hint::black_box;

use criterion::{BenchmarkId, Throughput};
use speedyspec_wp_shortcode::{HandAttsParser, HandScanner, RegexAttsParser};
use speedyspec_wp_shortcode_contracts::{AttsParser, ShortcodeScanner};

fn main() {
    let mut c = common::criterion();
    let scanner = HandScanner::new();
    let (hand, regex) = (HandAttsParser::new(), RegexAttsParser::new());
    let mut group = c.benchmark_group("atts");
    for (name, content) in common::corpus() {
        let tags = common::narrowed(&content);
        let texts: Vec<Vec<u8>> =
            scanner.scan(&content, &tags).map(|m| m.unwrap().groups_in(&content).0[3].clone()).collect();
        if texts.is_empty() {
            continue;
        }
        group.throughput(Throughput::Bytes(texts.iter().map(Vec::len).sum::<usize>() as u64));
        for (label, parser) in [("hand", &hand as &dyn AttsParser), ("regex", &regex)] {
            group.bench_with_input(BenchmarkId::new(label, &name), &texts, |b, texts| {
                b.iter(|| texts.iter().map(|t| parser.parse(black_box(t)).atts.iter().count()).sum::<usize>())
            });
        }
    }
    group.finish();
    c.final_summary();
}
