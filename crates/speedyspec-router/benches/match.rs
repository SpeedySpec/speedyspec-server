//! Matching and compiling WordPress rule lists: the router's two dispatch strategies against one regex per rule
//! (what WP::parse_request does in PHP), over WordPress 7.1.5's own `/%postname%/` rules (recorded in
//! compat/rewrite) and a 500-rule list (those rules plus generated `add_rewrite_rule` ones).

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use regex::Regex;
use speedyspec_http::Method;
use speedyspec_router::dispatch::{ChunkedAlternation, RegexSetStrategy};
use speedyspec_router::wordpress::php_urldecode;
use speedyspec_router::{CompiledTable, RouteRequest, Router, RuleSlot, WpSubject};
use speedyspec_routing::{DispatchStrategy, GuardAnswer, Rule, RuleGuard};

struct Accept;

impl RuleGuard for Accept {
    fn check(&self, _rule: &Rule, _captures: &[Option<String>]) -> GuardAnswer {
        GuardAnswer::Accept
    }
}

fn fixture() -> serde_json::Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../compat/rewrite/wordpress-7.1.5.json"
    );
    serde_json::from_str(&std::fs::read_to_string(path).expect("compat/rewrite fixture"))
        .expect("json")
}

/// The rules `get_option( 'rewrite_rules' )` returned after the postname structure's flush (step 3).
fn default_rules(fixture: &serde_json::Value) -> Vec<Rule> {
    fixture["results"]["structure_postname"][3]["returned"]["array"]
        .as_array()
        .expect("rules")
        .iter()
        .map(|pair| Rule {
            pattern: pair[0].as_str().unwrap().into(),
            query: pair[1].as_str().unwrap().into(),
        })
        .collect()
}

fn rules_500(defaults: &[Rule]) -> Vec<Rule> {
    let mut rules: Vec<Rule> = (0..500 - defaults.len())
        .map(|i| Rule {
            pattern: format!("plugin-{i}/([^/]+)/?$"),
            query: format!("index.php?plugin{i}=$matches[1]"),
        })
        .collect();
    rules.extend_from_slice(defaults);
    rules
}

fn corpus() -> Vec<String> {
    [
        "",
        "hello-world",
        "hello-world/2",
        "about",
        "parent/child",
        "category/news",
        "category/news/page/2",
        "tag/red",
        "author/admin",
        "2024/05/06",
        "feed",
        "hello-world/feed",
        "search/term",
        "robots.txt",
        "caf%C3%A9",
        "hello-world/attachment/img",
        "no/such/thing/here",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

fn table(rules: &[Rule], strategy: &dyn DispatchStrategy) -> CompiledTable {
    let mut router: Router<()> = Router::new();
    router.rules(RuleSlot::AfterRoutes, rules.to_vec());
    CompiledTable::compile(&router.data().unwrap(), strategy).unwrap()
}

fn resolve_all(table: &CompiledTable, corpus: &[String]) {
    for path in corpus {
        let request = RouteRequest {
            method: Method::Get,
            path: format!("/{path}"),
            wordpress: Some(WpSubject {
                requested_path: path.clone(),
                requested_file: path.clone(),
            }),
        };
        std::hint::black_box(table.resolve(&request, &Accept, &[], false));
    }
}

/// WordPress's loop: each rule, raw then decoded, until one matches.
fn one_per_rule(regexes: &[Regex], corpus: &[String]) {
    for path in corpus {
        let decoded = php_urldecode(path);
        std::hint::black_box(
            regexes
                .iter()
                .position(|re| re.is_match(path) || re.is_match(&decoded)),
        );
    }
}

fn bench(c: &mut Criterion) {
    let fixture = fixture();
    let defaults = default_rules(&fixture);
    let corpus = corpus();
    for (name, rules) in [
        ("default_rules", defaults.clone()),
        ("rules_500", rules_500(&defaults)),
    ] {
        let mut group = c.benchmark_group(name);
        let chunked = table(&rules, &ChunkedAlternation::default());
        let set = table(&rules, &RegexSetStrategy::default());
        let regexes: Vec<Regex> = rules
            .iter()
            .map(|r| Regex::new(&format!("^{}", r.pattern)).unwrap())
            .collect();
        group.bench_function(BenchmarkId::new("chunked", rules.len()), |b| {
            b.iter(|| resolve_all(&chunked, &corpus))
        });
        group.bench_function(BenchmarkId::new("regex_set", rules.len()), |b| {
            b.iter(|| resolve_all(&set, &corpus))
        });
        group.bench_function(BenchmarkId::new("one_per_rule", rules.len()), |b| {
            b.iter(|| one_per_rule(&regexes, &corpus))
        });
        group.finish();
    }
    let rules = rules_500(&defaults);
    let mut router: Router<()> = Router::new();
    router.rules(RuleSlot::AfterRoutes, rules);
    let data = router.data().unwrap();
    c.bench_function("compile_500", |b| {
        b.iter(|| {
            std::hint::black_box(
                CompiledTable::compile(&data, &ChunkedAlternation::default()).unwrap(),
            )
        })
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
