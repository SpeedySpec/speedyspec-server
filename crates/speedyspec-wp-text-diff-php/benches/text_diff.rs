//! `wp_text_diff()` on the recorded cases (`compat/text-diff/cases.json`). The SIMD scans are benchmarked in
//! speedyspec-platform, beside the crate.

use criterion::{Criterion, criterion_group, criterion_main};
use speedyspec_wp_text_diff::TextDiff;
use speedyspec_wp_text_diff::contracts::{NoLineFilter, RowLabels, TableOptions};
use std::hint::black_box;

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

criterion_group!(benches, cases);
criterion_main!(benches);
