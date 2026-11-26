//! WordPress's MIME list and filetype checks, replayed against the pinned release's recording
//! (`compat/media/recorded-mime.json`): every corpus file under its own name and under other extensions, through the
//! facade, with the in-process reader. Differences are listed in `compat/media/mime-differences.json`, each with a
//! reason in `DIFFERENCES.md`; WordPress's own list entries must come through unchanged.
//!
//! PHP boots once per process, so this test has its own `main()` (`harness = false`).

use std::path::Path;
use std::rc::Rc;

use serde_json::{Value, json};
use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, NativeArea, Php};
use speedyspec_wp_hooks_contracts::HookValue;
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::WpHooksArea;
use speedyspec_wp_media::SniffingReader;
use speedyspec_wp_media::mime::ADDED;
use speedyspec_wp_media_contracts::ProbeLimits;
use speedyspec_wp_media_ffmpeg::FfmpegReader;
use speedyspec_wp_media_midi::MidiReader;
use speedyspec_wp_media_php::MediaArea;

fn check(name: &str, ok: bool, detail: impl std::fmt::Display) {
    if ok {
        println!("ok: {name}");
    } else {
        eprintln!("FAIL: {name}: {detail}");
        std::process::exit(1);
    }
}

fn eval_json(php: &Php, name: &str, code: &str) -> Value {
    match php.eval(name, code) {
        Ok(HookValue::Str(s)) => serde_json::from_str(&s).expect("JSON from PHP"),
        other => panic!("{name}: {other:?}"),
    }
}

/// `[[key, value], …]` with the added rows' keys (or extensions) removed.
fn without_added(pairs: &Value, keys: &[&str]) -> Value {
    Value::Array(
        pairs
            .as_array()
            .unwrap()
            .iter()
            .filter(|p| !keys.contains(&p[0].as_str().unwrap()))
            .cloned()
            .collect(),
    )
}

fn main() {
    let compat = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../compat/media");
    let recorded: Value =
        serde_json::from_str(&std::fs::read_to_string(compat.join("recorded-mime.json")).unwrap())
            .unwrap();
    let differences: Vec<Value> = serde_json::from_str(
        &std::fs::read_to_string(compat.join("mime-differences.json")).unwrap(),
    )
    .unwrap();

    let release = speedyspec_compat::wordpress_release(&speedyspec_compat::pinned_wordpress())
        .expect("the pinned release");
    let release = speedyspec_wp_media_php::prepare_release(
        &release,
        &Path::new(env!("CARGO_TARGET_TMPDIR")).join("wordpress-media-mime"),
    )
    .expect("the release is prepared");
    let container = Rc::new(Container::new());
    let hooks = WpHooksArea::new(container.clone(), Rc::new(MemoryHookStore::new()));
    let limits = ProbeLimits::default();
    let media = MediaArea::new(hooks.api(), container.clone(), limits.clone());
    media.bind_defaults(Rc::new(SniffingReader::new(
        Box::new(MidiReader),
        Box::new(FfmpegReader::new(&limits)),
    )));
    let container_area = media.ports(ContainerArea::new("test", container.clone()));
    let areas: Vec<Rc<dyn NativeArea>> = vec![
        speedyspec_php_ext::twins_area(),
        Rc::new(container_area),
        Rc::new(hooks),
        Rc::new(media),
    ];
    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    })
    .expect("PHP boots");
    php.eval(
        "speedyspec://test/load",
        &format!(
            "define('ABSPATH', {:?}); define('WPINC', 'wp-includes'); error_reporting(E_ALL & ~E_DEPRECATED & ~E_WARNING & ~E_NOTICE);
             add_filter('pre_option', function ($pre, $option, $default) {{ return ['blog_charset' => 'UTF-8'][$option] ?? ($default === false ? '' : $default); }}, 10, 3);
             foreach (array('load.php', 'formatting.php', 'functions.php') as $f) {{ require ABSPATH . WPINC . '/' . $f; }}",
            format!("{}/", release.display())
        ),
    )
    .expect("WordPress's functions load");

    let lists = eval_json(
        &php,
        "speedyspec://test/lists",
        "$p = function ($a) { $o = array(); foreach ($a as $k => $v) { $o[] = array((string) $k, $v); } return $o; };
         return json_encode(array($p(wp_get_mime_types()), $p(wp_get_ext_types()), $p(get_allowed_mime_types())));",
    );
    let added_keys: Vec<&str> = ADDED.iter().map(|r| r.exts).collect();
    let added_exts: Vec<&str> = ADDED.iter().flat_map(|r| r.exts.split('|')).collect();
    let ext_types_without_added = Value::Array(
        lists[1]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| {
                let kept: Vec<Value> = p[1]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|e| {
                        !added_exts.contains(&e.as_str().unwrap()) || {
                            // An extension WordPress already files keeps its place.
                            recorded["wp_get_ext_types"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .any(|r| r[1].as_array().unwrap().contains(e))
                        }
                    })
                    .cloned()
                    .collect();
                json!([p[0], kept])
            })
            .collect(),
    );
    check(
        "wordpress_lists_are_kept",
        without_added(&lists[0], &added_keys) == recorded["wp_get_mime_types"]
            && ext_types_without_added == recorded["wp_get_ext_types"]
            && without_added(&lists[2], &added_keys) == recorded["get_allowed_mime_types"],
        "WordPress's entries changed",
    );

    // Every case at once: [file, name] → wp_check_filetype_and_ext()'s array.
    let cases: Vec<Value> = recorded["cases"].as_array().unwrap().clone();
    let input: Vec<Value> = cases
        .iter()
        .map(|c| {
            json!([
                compat
                    .join(c["file"].as_str().unwrap())
                    .display()
                    .to_string(),
                c["name"]
            ])
        })
        .collect();
    let input_path =
        std::env::temp_dir().join(format!("speedyspec-mime-{}.json", std::process::id()));
    std::fs::write(&input_path, serde_json::to_vec(&input).unwrap()).unwrap();
    let results = eval_json(
        &php,
        "speedyspec://test/cases",
        &format!(
            "$out = array();
             foreach (json_decode(file_get_contents({:?}), true) as $c) {{ $out[] = wp_check_filetype_and_ext($c[0], $c[1]); }}
             return json_encode($out);",
            input_path.display().to_string()
        ),
    );
    let _ = std::fs::remove_file(&input_path);

    let listed = |file: &str, name: &str| {
        differences
            .iter()
            .position(|d| d["file"] == file && d["name"] == name)
    };
    let mut used = vec![false; differences.len()];
    let mut mismatches = Vec::new();
    for (case, ours) in cases.iter().zip(results.as_array().unwrap()) {
        let file = case["file"].as_str().unwrap();
        let name = case["name"].as_str().unwrap();
        if *ours == case["result"] {
            continue;
        }
        match listed(file, name) {
            Some(i) => used[i] = true,
            None => mismatches.push(format!(
                "{file} as {name}: WordPress {}, ours {ours}",
                case["result"]
            )),
        }
    }
    check(
        "filetype_checks_match_wordpress_except_listed_differences",
        mismatches.is_empty(),
        format!(
            "{} mismatches:\n{}",
            mismatches.len(),
            mismatches.join("\n")
        ),
    );
    let stale: Vec<String> = differences
        .iter()
        .zip(&used)
        .filter(|(_, u)| !**u)
        .map(|(d, _)| format!("{} as {}", d["file"], d["name"]))
        .collect();
    check(
        "every_mime_difference_is_still_needed",
        stale.is_empty(),
        format!("entries that no longer differ:\n{}", stale.join("\n")),
    );
}
