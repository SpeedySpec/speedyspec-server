//! Every attack, bomb and corpus input, under each context: the port's output has no exploit.

mod common;

use speedyspec_wp_kses_contracts::{AllowedSpec, Context};
use speedyspec_wp_kses_security::{Level, inspect};
use speedyspec_wp_kses_testkit::{CORPUS_SETS, corpus};

#[test]
fn no_exploit_survives_kses() {
    let mut exploits = Vec::new();
    let mut policy = std::collections::BTreeMap::<&'static str, usize>::new();
    for set in CORPUS_SETS {
        for input in corpus(set) {
            for ctx in ["post", "data", "strip", "user_description"] {
                let (_, api) = common::setup();
                let context = Context::from_bytes(ctx.as_bytes());
                let out = api
                    .kses(&input, &AllowedSpec::Context(context.clone()), None)
                    .unwrap();
                let allowed = api.allowed_html(&context).unwrap();
                let protocols = api.allowed_protocols().unwrap();
                for f in inspect(&out, &allowed, &protocols, false) {
                    match f.level {
                        Level::Exploit => exploits.push(format!(
                            "{set} {ctx} {}: {} {}\n  in:  {}\n  out: {}",
                            f.rule,
                            f.rule,
                            f.detail,
                            common::short(&input),
                            common::short(&out)
                        )),
                        Level::Policy => *policy.entry(f.rule).or_default() += 1,
                    }
                }
            }
        }
    }
    let report = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../target/kses-policy-report.json"
    );
    let _ = std::fs::write(report, format!("{:?}\n", policy));
    eprintln!("policy findings by rule: {policy:?}");
    assert!(
        exploits.is_empty(),
        "{} exploits survive kses:\n{}",
        exploits.len(),
        exploits
            .iter()
            .take(25)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}
