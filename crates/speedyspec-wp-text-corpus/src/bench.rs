//! Bench inputs taken from the corpus, so Rust and PHP are timed on the same bytes.

use crate::cases;

/// The first string argument of every recorded call of `function`, labelled by scenario.
pub fn inputs(function: &str) -> Vec<(String, Vec<u8>)> {
    cases(function)
        .into_iter()
        .filter_map(|case| {
            let first = case.args.first()?.bytes()?.to_vec();
            Some((case.scenario, first))
        })
        .collect()
}

/// A long page for scaling: `function`'s whole-post inputs joined, repeated until it is at least 150 KB, the same
/// construction as the kses session's synthetic theme page.
pub fn page(function: &str) -> Vec<u8> {
    let posts: Vec<Vec<u8>> = inputs(function)
        .into_iter()
        .filter(|(scenario, _)| scenario.contains("/whole_posts/"))
        .map(|(_, bytes)| bytes)
        .collect();
    assert!(!posts.is_empty(), "the corpus has no whole posts for {function}");
    let mut page = Vec::new();
    while page.len() < 150 * 1024 {
        for post in &posts {
            page.extend_from_slice(post);
            page.push(b'\n');
        }
    }
    page
}
