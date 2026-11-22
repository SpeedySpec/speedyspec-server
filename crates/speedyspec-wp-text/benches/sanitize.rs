//! The sanitizers and `esc_url` over the corpus's short inputs (titles, file names, keys, addresses, URLs), in
//! bytes per second of thread CPU time, comparable with `php-baseline <function>`'s `short` group. Each function's
//! output on its single-argument corpus cases is checked against WordPress's before it is timed.

mod common;

use std::rc::Rc;

use common::{CpuTime, api};
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use speedyspec_hooks::Value;
use speedyspec_wp_text::title::{SanitizeContext, sanitize_title_with_dashes};
use speedyspec_wp_text::{FilterContext, TextApi, register_text_filters};
use speedyspec_wp_text_corpus::{Php, cases, from_value, short_inputs};

type Bench = Criterion<CpuTime>;

/// A function under test, from its first argument to what it returns.
type Run<'a> = &'a dyn Fn(&[u8]) -> Php;

fn bench(c: &mut Bench, function: &str, run: Run<'_>) {
    for case in cases(function).iter().filter(|c| c.args.len() == 1 && c.args[0].bytes().is_some()) {
        assert_eq!(run(case.args[0].bytes().unwrap()), case.returned, "{}", case.scenario);
    }
    let inputs = short_inputs(function);
    let mut group = c.benchmark_group(function);
    group.throughput(Throughput::Bytes(inputs.iter().map(Vec::len).sum::<usize>() as u64));
    group.bench_function("short", |b| {
        b.iter(|| {
            for input in &inputs {
                std::hint::black_box(run(std::hint::black_box(input)));
            }
        })
    });
    group.finish();
}

fn string(bytes: &[u8]) -> Value {
    speedyspec_wp_text::strings::bytes_value(bytes.to_vec())
}

fn sanitize(c: &mut Bench) {
    let api: Rc<TextApi> = Rc::new(api());
    register_text_filters(&api, &api.deps().hooks, FilterContext { admin: false }).unwrap();
    bench(c, "esc_url", &|t| from_value(&api.esc_url(t, None, b"display").unwrap()));
    bench(c, "sanitize_title", &|t| from_value(&api.sanitize_title(&string(t), b"", &SanitizeContext::Save).unwrap()));
    bench(c, "sanitize_title_with_dashes", &|t| Php::Str(sanitize_title_with_dashes(t, &SanitizeContext::Display)));
    bench(c, "sanitize_file_name", &|t| from_value(&api.sanitize_file_name(t).unwrap()));
    bench(c, "sanitize_key", &|t| from_value(&api.sanitize_key(&string(t)).unwrap()));
    bench(c, "sanitize_text_field", &|t| from_value(&api.sanitize_text_field(&string(t)).unwrap()));
    bench(c, "sanitize_html_class", &|t| from_value(&api.sanitize_html_class(t, b"").unwrap()));
    bench(c, "is_email", &|t| from_value(&api.is_email(&string(t), &Value::Bool(false)).unwrap()));
    bench(c, "sanitize_email", &|t| from_value(&api.sanitize_email(t).unwrap()));
    bench(c, "remove_accents", &|t| Php::Str(api.remove_accents(t, None).unwrap().into_owned()));
}

criterion_group! {
    name = benches;
    config = Criterion::default().with_measurement(CpuTime);
    targets = sanitize
}
criterion_main!(benches);
