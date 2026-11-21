//! Benchmarks: each SIMD scan against its scalar twin, and `wp_text_diff()` on the recorded cases.

use criterion::{Criterion, criterion_group, criterion_main};
use speedyspec_wp_text_diff::contracts::{NoLineFilter, RowLabels, TableOptions};
use speedyspec_wp_text_diff::{TextDiff, html_special_chars, simd, split_words};
use std::hint::black_box;

fn text() -> Vec<u8> {
    let mut s = Vec::new();
    for i in 0..2000 {
        s.extend_from_slice(b"The quick brown fox jumps over the lazy dog, again and again ");
        if i % 50 == 0 {
            s.extend_from_slice(b"<b>&amp;</b> ");
        }
    }
    s
}

fn scans(c: &mut Criterion) {
    let s = text();
    c.bench_function("find_escaped/simd", |b| {
        b.iter(|| simd::find_escaped(black_box(&s[..4000])))
    });
    c.bench_function("find_escaped/scalar", |b| {
        b.iter(|| simd::scalar::find_escaped(black_box(&s[..4000])))
    });
    let words =
        b"Supercalifragilisticexpialidocious_and_more_words_without_spaces_at_all_here!".repeat(20);
    c.bench_function("ascii_word_run/simd", |b| {
        b.iter(|| simd::ascii_word_run(black_box(&words)))
    });
    c.bench_function("ascii_word_run/scalar", |b| {
        b.iter(|| simd::scalar::ascii_word_run(black_box(&words)))
    });
    c.bench_function("html_special_chars", |b| {
        b.iter(|| html_special_chars(black_box(&s)))
    });
    c.bench_function("split_words", |b| {
        b.iter(|| split_words(black_box(&s)).map(|w| w.pieces().len()))
    });
}

fn cases(c: &mut Criterion) {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../compat/text-diff/cases.json"
    );
    let cases: Vec<serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let pairs: Vec<(String, String)> = cases
        .iter()
        .filter(|c| c["left"].is_string() && c["right"].is_string() && c.get("filter").is_none())
        .map(|c| {
            (
                c["left"].as_str().unwrap().to_owned(),
                c["right"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    let diff = TextDiff::default();
    let (options, labels) = (TableOptions::default(), RowLabels::default());
    c.bench_function("wp_text_diff/recorded_cases", |b| {
        b.iter(|| {
            for (l, r) in &pairs {
                let _ = black_box(diff.table(
                    l.as_bytes(),
                    r.as_bytes(),
                    &options,
                    &labels,
                    &NoLineFilter,
                ));
            }
        })
    });
}

criterion_group!(benches, scans, cases);
criterion_main!(benches);
