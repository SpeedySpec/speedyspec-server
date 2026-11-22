//! Construction, merging and `get_stylesheet` (each type) for WordPress 7.1.5's core `theme.json` and default
//! themes, over every core block. Run: `cargo bench -p speedyspec-wp-theme-json --bench stylesheet`.

#[path = "../tests/support/mod.rs"]
#[allow(dead_code, unused_imports)]
mod support;

use std::rc::Rc;

use criterion::{Criterion, criterion_group, criterion_main};
use speedyspec_hooks::Value;
use speedyspec_wp_theme_json::ThemeJsonApi;
use speedyspec_wp_theme_json_contracts::ports::RegisteredBlock;

const CORE: &str = include_str!("data/core.json");
const THEMES: [(&str, &str); 3] = [
    (
        "twentytwentythree",
        include_str!("data/twentytwentythree.json"),
    ),
    (
        "twentytwentyfour",
        include_str!("data/twentytwentyfour.json"),
    ),
    (
        "twentytwentyfive",
        include_str!("data/twentytwentyfive.json"),
    ),
];

fn core_blocks() -> Vec<RegisteredBlock> {
    let Value::Array(items) = support::json(include_str!("data/core-blocks.json")) else {
        unreachable!()
    };
    items
        .into_iter()
        .map(|(_, b)| {
            let get = |k: &str| {
                speedyspec_wp_theme_json::array_path::get(&b, &[k])
                    .cloned()
                    .unwrap_or(Value::Null)
            };
            let Value::Str(name) = get("name") else {
                unreachable!()
            };
            let styles = match get("styles") {
                Value::Array(s) => s
                    .into_iter()
                    .filter_map(|(_, v)| if let Value::Str(s) = v { Some(s) } else { None })
                    .collect(),
                _ => Vec::new(),
            };
            RegisteredBlock {
                name,
                selectors: get("selectors"),
                supports: get("supports"),
                style_names: styles,
            }
        })
        .collect()
}

fn bench(c: &mut Criterion) {
    let (deps, _) = support::deps(core_blocks());
    let api = Rc::new(ThemeJsonApi::new(deps));
    let core = support::json(CORE);
    let core_doc = api.construct(core.clone(), b"default");
    let types = |t: &str| support::json(&format!("[\"{t}\"]"));
    let options = support::json(r#"{"include_block_style_variations":true}"#);
    for (name, text) in THEMES {
        let raw = support::json(text);
        c.bench_function(&format!("{name}/construct"), |b| {
            b.iter(|| api.construct(raw.clone(), b"theme"))
        });
        let theme = api.construct(raw.clone(), b"theme");
        c.bench_function(&format!("{name}/merge_into_core"), |b| {
            b.iter(|| api.merge(&core_doc, &theme))
        });
        let merged = api.merge(&core_doc, &theme);
        for t in ["variables", "styles", "presets", "custom-css"] {
            let types = types(t);
            c.bench_function(&format!("{name}/get_stylesheet/{t}"), |b| {
                b.iter(|| {
                    api.get_stylesheet(&merged, &types, &Value::Null, &options)
                        .unwrap()
                })
            });
        }
    }
    c.bench_function("core/construct", |b| {
        b.iter(|| api.construct(core.clone(), b"default"))
    });
    let all = support::json(r#"["variables","styles","presets"]"#);
    c.bench_function("core/get_stylesheet/default", |b| {
        b.iter(|| {
            api.get_stylesheet(&core_doc, &all, &Value::Null, &options)
                .unwrap()
        })
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
