//! `wp_kses_post` throughput on the design's corpus, against its goals: at least 250 MB/s on post snippets and
//! 500 MB/s on a full page. Run with `cargo bench -p speedyspec-wp-kses`; `KSES_BENCH_ITERATIONS` changes the 200
//! iterations. Times are the thread's CPU time, so a busy machine skews them less.

use std::rc::Rc;

use speedyspec_wp_hooks::{HookApi, IgnoreDeprecations};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_kses::{DefaultGlobals, KsesApi, KsesDeps, NoValueCallbacks, UploadsLocation};
use speedyspec_wp_kses_contracts::{
    AttributeEscaper, EntityContext, KsesError, UnfilteredHtmlPolicy,
};
use speedyspec_wp_kses_testkit::{bench_page, bench_snippets};

/// `esc_attr()` as WordPress behaves with no filter on valid UTF-8 (the same as the tests' double).
struct Escaper;
impl AttributeEscaper for Escaper {
    fn esc_attr(&self, v: &[u8]) -> Result<Vec<u8>, KsesError> {
        if std::str::from_utf8(v).is_err() {
            return Ok(Vec::new());
        }
        let normalized = speedyspec_wp_kses_lexical::normalize_entities(
            v,
            EntityContext::Html,
            speedyspec_wp_kses_lexical::default_html_entity_names(),
            speedyspec_wp_kses_lexical::default_xml_entity_names(),
        );
        let mut out = Vec::with_capacity(normalized.len());
        for b in normalized.iter() {
            match b {
                b'<' => out.extend_from_slice(b"&lt;"),
                b'>' => out.extend_from_slice(b"&gt;"),
                b'"' => out.extend_from_slice(b"&quot;"),
                b'\'' => out.extend_from_slice(b"&#039;"),
                b => out.push(*b),
            }
        }
        Ok(out)
    }
}

struct Policy;
impl UnfilteredHtmlPolicy for Policy {
    fn can_unfiltered_html(&self) -> Result<bool, KsesError> {
        Ok(false)
    }
}

struct Uploads;
impl UploadsLocation for Uploads {
    fn upload_url(&self) -> Result<Vec<u8>, KsesError> {
        Ok(b"http://example.org/wp-content/uploads".to_vec())
    }
}

/// This thread's CPU time in seconds: other processes on a busy machine do not count against kses.
fn cpu_seconds() -> f64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `ts` is a valid timespec for the call to fill.
    unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) };
    ts.tv_sec as f64 + ts.tv_nsec as f64 / 1e9
}

/// Bytes per CPU second over `docs`, the best of five rounds of `iterations` passes each.
fn throughput(api: &KsesApi, docs: &[Vec<u8>], iterations: usize) -> (f64, f64) {
    let bytes: usize = docs.iter().map(Vec::len).sum();
    let mut best = f64::MAX;
    for _ in 0..5 {
        let start = cpu_seconds();
        for _ in 0..iterations {
            for d in docs {
                std::hint::black_box(api.post(std::hint::black_box(d)).unwrap());
            }
        }
        best = best.min(cpu_seconds() - start);
    }
    let mb_s = (bytes * iterations) as f64 / best / 1e6;
    let us_doc = best / (iterations * docs.len()) as f64 * 1e6;
    (mb_s, us_doc)
}

fn main() {
    let iterations = std::env::var("KSES_BENCH_ITERATIONS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200);
    let hooks = Rc::new(HookApi::new(
        Rc::new(MemoryHookStore::new()),
        Rc::new(IgnoreDeprecations),
    ));
    let api = KsesApi::new(KsesDeps {
        hooks,
        escaper: Rc::new(Escaper),
        policy: Rc::new(Policy),
        globals: Rc::new(DefaultGlobals::default()),
        uploads: Rc::new(Uploads),
        callbacks: Rc::new(NoValueCallbacks),
        html_api: Rc::new(speedyspec_wp_kses::NoHtmlApiSanitizer),
        mode: Rc::new(speedyspec_wp_kses::MemoryOperatingMode::default()),
    });
    let snippets = bench_snippets();
    let page = vec![bench_page()];
    let rows = [
        ("snippets", &snippets, iterations, 250.0),
        ("page", &page, iterations, 500.0),
    ];
    println!(
        "{:<10} {:>6} {:>10} {:>10} {:>8} {:>6}",
        "corpus", "docs", "MB/s", "µs/doc", "goal", "met"
    );
    let mut missed = false;
    for (name, docs, n, goal) in rows {
        let (mb_s, us) = throughput(&api, docs, n);
        let met = mb_s >= goal;
        missed |= !met;
        println!(
            "{name:<10} {:>6} {mb_s:>10.1} {us:>10.2} {goal:>8.0} {:>6}",
            docs.len(),
            if met { "yes" } else { "NO" }
        );
    }
    if missed {
        std::process::exit(1);
    }
}
