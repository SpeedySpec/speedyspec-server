//! Times each recorded case, slowest first, to find where `wp_text_diff()` spends its time.
use speedyspec_wp_text_diff::TextDiff;
use speedyspec_wp_text_diff::contracts::{NoLineFilter, RowLabels, TableOptions};
fn main() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../compat/text-diff/cases.json"
    );
    let cases: Vec<serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let diff = TextDiff::default();
    let mut times = Vec::new();
    for c in &cases {
        let (Some(l), Some(r)) = (c["left"].as_str(), c["right"].as_str()) else {
            continue;
        };
        let t = std::time::Instant::now();
        for _ in 0..50 {
            let _ = std::hint::black_box(diff.table(
                l.as_bytes(),
                r.as_bytes(),
                &TableOptions::default(),
                &RowLabels::default(),
                &NoLineFilter,
            ));
        }
        times.push((
            t.elapsed() / 50,
            c["name"].as_str().unwrap().to_owned(),
            l.lines().count(),
            r.lines().count(),
            l.len(),
        ));
    }
    let total: std::time::Duration = times.iter().map(|t| t.0).sum();
    times.sort_by_key(|t| std::cmp::Reverse(t.0));
    eprintln!("total {total:?}");
    for t in times.iter().take(8) {
        eprintln!(
            "{:>10?} {} ({} -> {} lines, {} bytes)",
            t.0, t.1, t.2, t.3, t.4
        );
    }
}
