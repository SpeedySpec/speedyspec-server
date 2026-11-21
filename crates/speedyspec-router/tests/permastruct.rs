//! WordPress 7.1.5's generate_rewrite_rules() output, recorded by fixtures/permastruct/record.php, replayed.

use serde_json::Value;
use speedyspec_router::wordpress::{
    ExpandContext, generate_rewrite_rule, generate_rewrite_rules, preg_index,
};
use speedyspec_routing::{Endpoint, EndpointMask, PermastructArgs, RewriteTag, Rule};

fn cases() -> Vec<Value> {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/permastruct/cases.json"
    ))
    .expect("fixture");
    serde_json::from_str::<Value>(&text)
        .expect("json")
        .as_array()
        .expect("array")
        .clone()
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

fn check(name: &str) {
    let case = cases()
        .into_iter()
        .find(|c| c["name"] == name)
        .unwrap_or_else(|| panic!("no case {name}"));
    let tags: Vec<RewriteTag> = case["tags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| RewriteTag {
            tag: t[0].as_str().unwrap().into(),
            regex: t[1].as_str().unwrap().into(),
            query: t[2].as_str().unwrap().into(),
        })
        .collect();
    let endpoints: Vec<Endpoint> = case["endpoints"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| Endpoint {
            mask: EndpointMask(e[0].as_u64().unwrap() as u32),
            name: e[1].as_str().unwrap().into(),
            query_var: e[2].as_str().unwrap().into(),
        })
        .collect();
    let feeds = strings(&case["feeds"]);
    let post_types: Vec<(String, bool)> = case["post_types"]
        .as_object()
        .map(|o| {
            o.iter()
                .map(|(k, v)| (k.clone(), v.as_bool().unwrap()))
                .collect()
        })
        .unwrap_or_default();
    let ctx = ExpandContext {
        tags: &tags,
        endpoints: &endpoints,
        feeds: &feeds,
        index: "index.php",
        feed_base: "feed",
        pagination_base: "page",
        comments_pagination_base: "comment-page",
        matches: case["matches"].as_str().unwrap(),
        page_on_front: case["page_on_front"].as_str().unwrap(),
        custom_post_types: &post_types,
    };
    let structure = case["struct"].as_str().unwrap();
    let actual = if case["rule"].as_bool().unwrap() {
        generate_rewrite_rule(structure, false, &ctx)
    } else {
        let args = PermastructArgs {
            with_front: true,
            ep_mask: EndpointMask(case["ep_mask"].as_u64().unwrap() as u32),
            paged: case["paged"].as_bool().unwrap(),
            feed: case["feed"].as_bool().unwrap(),
            forcomments: case["forcomments"].as_bool().unwrap(),
            walk_dirs: case["walk_dirs"].as_bool().unwrap(),
            endpoints: true,
        };
        generate_rewrite_rules(structure, &args, &ctx)
    };
    let expected: Vec<Rule> = case["rules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| Rule {
            pattern: r[0].as_str().unwrap().into(),
            query: r[1].as_str().unwrap().into(),
        })
        .collect();
    assert_eq!(actual, expected, "case {name}");
}

#[test]
fn postname_structure() {
    check("postname_structure");
}

#[test]
fn date_structure() {
    check("date_structure");
}

#[test]
fn category_with_endpoints() {
    check("category_with_endpoints");
}

#[test]
fn forcomments() {
    check("forcomments");
}

#[test]
fn walk_dirs_false() {
    check("walk_dirs_false");
}

#[test]
fn index_permalinks() {
    check("index_permalinks");
}

#[test]
fn no_trailing_slash() {
    check("no_trailing_slash");
}

#[test]
fn root_with_page_on_front() {
    check("root_with_page_on_front");
}

#[test]
fn pagename() {
    check("pagename");
}

#[test]
fn attachment_endpoint_on_post() {
    check("attachment_endpoint_on_post");
}

#[test]
fn custom_post_type() {
    check("custom_post_type");
}

#[test]
fn hierarchical_custom_post_type() {
    check("hierarchical_custom_post_type");
}

#[test]
fn generate_rewrite_rule_has_no_extras() {
    check("generate_rewrite_rule");
}

#[test]
fn no_tags() {
    check("no_tags");
}

#[test]
fn plain_matches() {
    check("plain_matches");
}

#[test]
fn preg_index_forms() {
    assert_eq!(preg_index(3, "matches"), "$matches[3]");
    assert_eq!(preg_index(3, ""), "$3");
}

#[test]
fn every_recorded_case_has_a_test() {
    let names: Vec<String> = cases()
        .iter()
        .map(|c| c["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(names.len(), 15);
}
