//! Every corpus input, under `post`, `data` and `strip`, against WordPress 7.1.5's kses. A difference passes only
//! by the gate rule, and every security break must be listed in compat/kses/breaks.json.

mod common;

use serde_json::Value as Json;
use sha2::{Digest, Sha256};
use speedyspec_wp_kses_contracts::{AllowedSpec, Context};
use speedyspec_wp_kses_testkit::{
    CORPUS_SETS, PhpOracle, Verdict, bytes, corpus, from_bytes, gate,
};

pub fn listed_breaks() -> Vec<String> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../compat/kses/breaks.json");
    let json: Json = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    json["breaks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["scenario"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn corpus_matches_php() {
    let breaks = listed_breaks();
    let mut php = PhpOracle::start().unwrap();
    let mut failures = Vec::new();
    let mut unlisted = Vec::new();
    for set in CORPUS_SETS {
        for input in corpus(set) {
            let id = format!("{:x}", Sha256::digest(&input));
            for ctx in ["post", "data", "strip"] {
                let (_, api) = common::setup();
                let expected = from_bytes(
                    &php.call("wp_kses", vec![bytes(&input), Json::String(ctx.into())])
                        .unwrap()
                        .returned,
                );
                let ours = api
                    .kses(
                        &input,
                        &AllowedSpec::Context(Context::from_bytes(ctx.as_bytes())),
                        None,
                    )
                    .unwrap();
                match gate(&expected, &ours, common::exploit_finder().as_deref()) {
                    Verdict::Same => {}
                    Verdict::SecurityBreak(_) => {
                        let scenario = format!("corpus:{set}:{}:{ctx}", &id[..16]);
                        if !breaks.contains(&scenario) {
                            unlisted.push(scenario);
                        }
                    }
                    Verdict::Fail(why) => {
                        failures.push(format!("{set} {ctx} {}: {why}", &id[..16]))
                    }
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} differences:\n{}",
        failures.len(),
        failures
            .iter()
            .take(10)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n\n")
    );
    assert!(
        unlisted.is_empty(),
        "security breaks missing from breaks.json: {unlisted:?}"
    );
}
