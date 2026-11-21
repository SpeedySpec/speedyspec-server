//! `esc_html`, `esc_attr` and `esc_textarea` through `EscapeApi` (no filters registered), over the corpus's whole
//! posts, a long page and the short inputs, comparable with `php-baseline <function>`. Each function's output on
//! every timed input is checked against WordPress's first. Times are thread CPU time.

use std::rc::Rc;
use std::time::Duration;

use criterion::measurement::{Measurement, ValueFormatter, WallTime};
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use speedyspec_hooks::Value;
use speedyspec_wp_hooks::{HookApi, IgnoreDeprecations};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_text_corpus::{cases, from_value, page, short_inputs};
use speedyspec_wp_text_escape::EscapeApi;
use speedyspec_wp_text_memory::{FixedSiteSettings, RecordingDiagnostics};

/// The calling thread's CPU time.
fn thread_cpu() -> Duration {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: clock_gettime writes one timespec through a valid pointer.
    unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) };
    Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32)
}

/// Criterion's measurement as thread CPU time.
struct CpuTime;

static WALL: WallTime = WallTime;

impl Measurement for CpuTime {
    type Intermediate = Duration;
    type Value = Duration;

    fn start(&self) -> Duration {
        thread_cpu()
    }

    fn end(&self, start: Duration) -> Duration {
        thread_cpu() - start
    }

    fn add(&self, a: &Duration, b: &Duration) -> Duration {
        *a + *b
    }

    fn zero(&self) -> Duration {
        Duration::ZERO
    }

    fn to_f64(&self, value: &Duration) -> f64 {
        value.as_nanos() as f64
    }

    fn formatter(&self) -> &dyn ValueFormatter {
        WALL.formatter()
    }
}

fn escape(c: &mut Criterion<CpuTime>) {
    let api = EscapeApi::new(
        Rc::new(HookApi::new(Rc::new(MemoryHookStore::new()), Rc::new(IgnoreDeprecations))),
        Rc::new(FixedSiteSettings::default()),
        Rc::new(RecordingDiagnostics::default()),
    );
    let functions: [(&str, &dyn Fn(&[u8]) -> Value); 3] = [
        ("esc_html", &|t| api.esc_html(t).unwrap()),
        ("esc_attr", &|t| api.esc_attr(t).unwrap()),
        ("esc_textarea", &|t| api.esc_textarea(t).unwrap()),
    ];
    for (function, run) in functions {
        for case in cases(function) {
            assert_eq!(from_value(&run(case.args[0].bytes().unwrap())), case.returned, "{}", case.scenario);
        }
        let posts: Vec<Vec<u8>> = cases(function)
            .into_iter()
            .filter(|c| c.scenario.contains("/whole_posts/"))
            .filter_map(|c| c.args[0].bytes().map(<[u8]>::to_vec))
            .collect();
        let page = page(function);
        let short = short_inputs(function);
        let mut group = c.benchmark_group(function);
        for (name, items) in [("whole_posts", posts), ("page", vec![page]), ("short", short)] {
            group.throughput(Throughput::Bytes(items.iter().map(Vec::len).sum::<usize>() as u64));
            group.bench_function(name, |b| {
                b.iter(|| {
                    for item in &items {
                        std::hint::black_box(run(std::hint::black_box(item)));
                    }
                })
            });
        }
        group.finish();
    }
}

criterion_group! {
    name = benches;
    config = Criterion::default().with_measurement(CpuTime);
    targets = escape
}
criterion_main!(benches);
