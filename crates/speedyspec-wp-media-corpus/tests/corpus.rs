//! Every file in `compat/media/corpus` read with FFmpeg (or the MIDI reader) through WordPress's rules must match what
//! WordPress 7.1.5 and getID3 returned (`compat/media/recorded.json`), except the paths
//! `compat/media/differences.json` lists, each with a reason in `DIFFERENCES.md`.

mod common;

fn run() -> (Vec<String>, Vec<String>) {
    common::run("recorded.json", "corpus", "differences.json", None)
}

#[test]
fn corpus_matches_wordpress_except_listed_differences() {
    let (mismatches, _) = run();
    assert!(
        mismatches.is_empty(),
        "{} mismatches:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
}

#[test]
fn every_difference_is_still_needed() {
    let (_, stale) = run();
    assert!(
        stale.is_empty(),
        "differences.json entries that no longer differ:\n{}",
        stale.join("\n")
    );
}
