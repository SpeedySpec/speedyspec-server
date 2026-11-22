//! Bombs aimed at kses itself: time and allocation grow at most a little faster than the input (n log n, with
//! slack), output stays within a fixed multiple of the input, and nothing overflows a small stack.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::time::Instant;

use speedyspec_wp_kses_contracts::{AllowedSpec, Context};
use speedyspec_wp_kses_testkit::{BombGenerator, bomb_generators};

/// Counts the bytes each thread allocates, so allocation growth can be compared across sizes. Per thread, so tests
/// running in parallel do not count each other's allocations.
struct Counting;

thread_local! {
    static ALLOCATED: Cell<usize> = const { Cell::new(0) };
}

/// The bytes this thread has allocated so far.
fn allocated() -> usize {
    ALLOCATED.with(Cell::get)
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // `try_with`: a thread's own allocations while it tears down its locals go uncounted.
        let _ = ALLOCATED.try_with(|a| a.set(a.get() + layout.size()));
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// `&` becomes `&amp;`: the largest growth kses applies to any byte.
const OUTPUT_MULTIPLE: usize = 5;

fn measure(input: &[u8]) -> (f64, usize, usize) {
    let input = input.to_vec();
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || {
            let (_, api) = common::setup();
            let mut times = Vec::new();
            let mut bytes = 0;
            let mut out_len = 0;
            for _ in 0..5 {
                let before = allocated();
                let start = Instant::now();
                let out = api
                    .kses(&input, &AllowedSpec::Context(Context::Post), None)
                    .unwrap();
                times.push(start.elapsed().as_secs_f64());
                bytes = allocated() - before;
                out_len = out.len();
            }
            times.sort_by(|a, b| a.partial_cmp(b).unwrap());
            (times[2], bytes, out_len)
        })
        .unwrap()
        .join()
        .unwrap()
}

/// Timing needs an optimized build: run with `cargo test --release`.
#[test]
#[cfg_attr(debug_assertions, ignore = "timing needs --release")]
fn bombs_grow_at_most_n_log_n() {
    let n = 64 * 1024;
    let limit = 4.0 * ((4 * n) as f64).log2() / (n as f64).log2() * 1.5;
    let mut problems = Vec::new();
    for (name, generate) in bomb_generators() {
        let (small, large) = (generate(n), generate(4 * n));
        let _ = measure(&generate(2 * n));
        let (t1, a1, o1) = measure(&small);
        let (t4, a4, o4) = measure(&large);
        for (input, out) in [(&small, o1), (&large, o4)] {
            if out > OUTPUT_MULTIPLE * input.len() + 16 {
                problems.push(format!("{name}: output {out} for input {}", input.len()));
            }
        }
        let time_ratio = t4 / t1.max(1e-6);
        let alloc_ratio = a4 as f64 / (a1.max(1)) as f64;
        if time_ratio > limit && t4 > 0.005 {
            problems.push(format!(
                "{name}: time x{time_ratio:.1} for 4x input ({t1:.4}s -> {t4:.4}s)"
            ));
        }
        if alloc_ratio > limit {
            problems.push(format!(
                "{name}: allocation x{alloc_ratio:.1} for 4x input ({a1} -> {a4} bytes)"
            ));
        }
    }
    assert!(
        problems.is_empty(),
        "limit x{limit:.2}:\n{}",
        problems.join("\n")
    );
}

#[test]
#[cfg_attr(debug_assertions, ignore = "timing needs --release")]
fn huge_inputs_stay_linear() {
    let shapes: [BombGenerator; 2] = [
        ("unterminated comment", |n| {
            [b"<p>x</p>".repeat(n / 8), b"<!--".to_vec()].concat()
        }),
        ("one huge attribute value", |n| {
            [
                b"<p title=\"".to_vec(),
                b"a".repeat(n),
                b"\">x</p>".to_vec(),
            ]
            .concat()
        }),
    ];
    for (name, shape) in shapes {
        let (t1, _, o1) = measure(&shape(1 << 20));
        let big = if name.starts_with("unterminated") {
            50 << 20
        } else {
            10 << 20
        };
        let input = shape(big);
        let (t, _, o) = measure(&input);
        assert!(
            t <= 20.0 * t1 * (big >> 20) as f64 / 1.0 + 0.5,
            "{name}: {t:.3}s at {} MB vs {t1:.4}s at 1 MB",
            big >> 20
        );
        assert!(
            o <= OUTPUT_MULTIPLE * input.len() + 16 && o1 > 0,
            "{name}: output {o}"
        );
    }
}
