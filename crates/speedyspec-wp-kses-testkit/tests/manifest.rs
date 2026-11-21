use std::path::Path;

use sha2::{Digest, Sha256};

fn corpus() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/corpus"))
}

#[test]
fn manifest_matches_files() {
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(corpus().join("MANIFEST.json")).expect("MANIFEST.json"),
    )
    .unwrap();
    let entries = manifest.as_array().unwrap();
    let mut listed = Vec::new();
    for e in entries {
        let file = e["file"].as_str().unwrap();
        let bytes = std::fs::read(corpus().join(file))
            .unwrap_or_else(|_| panic!("{file} is listed but missing"));
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes)),
            e["sha256"].as_str().unwrap(),
            "{file}"
        );
        for key in ["source", "license"] {
            assert!(
                !e[key].as_str().unwrap_or_default().is_empty(),
                "{file} has no {key}"
            );
        }
        listed.push(file.to_string());
    }
    for dir in ["attacks", "bombs"] {
        for f in std::fs::read_dir(corpus().join(dir)).unwrap() {
            let name = format!("{dir}/{}", f.unwrap().file_name().to_string_lossy());
            if name.ends_with(".json") || dir == "bombs" && !name.ends_with(".md") {
                assert!(listed.contains(&name), "{name} is not in MANIFEST.json");
            }
        }
    }
}

#[test]
fn no_portswigger_data_is_committed() {
    for entry in walk(corpus()) {
        let name = entry.to_string_lossy().to_ascii_lowercase();
        assert!(
            !name.contains("portswigger") && !name.contains("port_swigger"),
            "{name}"
        );
    }
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p))
        } else {
            out.push(p)
        }
    }
    out
}

#[test]
fn attack_and_bomb_sets_load() {
    assert!(speedyspec_wp_kses_testkit::corpus("attacks").len() > 4000);
    assert!(speedyspec_wp_kses_testkit::corpus("bombs").len() >= 25);
    for (name, generate) in speedyspec_wp_kses_testkit::bomb_generators() {
        assert!(generate(1000).len() >= 900, "{name}: about n bytes");
    }
}
