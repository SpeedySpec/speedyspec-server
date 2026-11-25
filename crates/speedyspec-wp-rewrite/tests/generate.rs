//! WordPress's rewrite_rules(), mod_rewrite_rules() and iis7_url_rewrite_rules(), recorded from the rewrite area's
//! pinned release by fixtures/rules/record.php, replayed; and the hooks generation fires.

mod support;

use serde_json::Value;
use speedyspec_routing::{Endpoint, EndpointMask, PermastructArgs, Rule, RulePriority};
use speedyspec_wp_hooks::{Callback, HookName, HookValue};
use speedyspec_wp_rewrite::{
    QueryableTaxonomy, RandomablePostType, RewriteState, RewriteStateSource, RuleQuery,
};
use support::{Fixture, fixture_with};

fn cases() -> Vec<Value> {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/rules/cases.json"
    ))
    .unwrap();
    serde_json::from_str::<Value>(&text)
        .unwrap()
        .as_array()
        .unwrap()
        .clone()
}

fn rules(value: &Value) -> Vec<Rule> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|r| Rule {
            pattern: r[0].as_str().unwrap().into(),
            query: r[1].as_str().unwrap().into(),
        })
        .collect()
}

/// The fixture and state for a recorded case, with the taxonomies' tags and permastructs registered.
fn setup(case: &Value) -> (Fixture, RewriteState) {
    let options = case["options"].as_object().unwrap();
    let f = fixture_with(
        options["permalink_structure"].as_str().unwrap(),
        case["home"].as_str().unwrap(),
        case["site"].as_str().unwrap(),
    );
    for (name, value) in options {
        f.options.set(name, value.as_str().unwrap());
    }
    let mut s = RewriteState::default();
    f.service.init(&mut s).unwrap();
    s.matches = "matches".into();
    s.use_verbose_rules = case["verbose"].as_bool().unwrap();
    f.service
        .add_rewrite_tag(&mut s, "%category%", "(.+?)", "category_name=");
    f.service
        .add_rewrite_tag(&mut s, "%post_tag%", "([^/]+)", "tag=");
    f.service
        .add_rewrite_tag(&mut s, "%post_format%", "([^/]+)", "post_format=");
    let base = |key: &str, default: &str| {
        options
            .get(key)
            .and_then(|v| v.as_str())
            .filter(|v| !v.is_empty())
            .unwrap_or(default)
            .to_string()
    };
    let with_mask = |mask| PermastructArgs {
        ep_mask: mask,
        ..PermastructArgs::default()
    };
    let category = format!("{}/%category%", base("category_base", "category"));
    let tag = format!("{}/%post_tag%", base("tag_base", "tag"));
    f.service.add_permastruct(
        &mut s,
        "category",
        &category,
        with_mask(EndpointMask::CATEGORIES),
    );
    f.service
        .add_permastruct(&mut s, "post_tag", &tag, with_mask(EndpointMask::TAGS));
    f.service.add_permastruct(
        &mut s,
        "post_format",
        "type/%post_format%",
        PermastructArgs::default(),
    );
    for p in case["permastructs"].as_array().unwrap() {
        let name = p[0].as_str().unwrap();
        f.service.add_rewrite_tag(
            &mut s,
            &format!("%{name}%"),
            p[2].as_str().unwrap(),
            p[3].as_str().unwrap(),
        );
        f.service.add_permastruct(
            &mut s,
            name,
            p[1].as_str().unwrap(),
            PermastructArgs::default(),
        );
    }
    random_content(&f, case);
    for e in case["endpoints"].as_array().unwrap() {
        s.endpoints.push(Endpoint {
            mask: EndpointMask(e[0].as_u64().unwrap() as u32),
            name: e[1].as_str().unwrap().into(),
            query_var: e[2].as_str().unwrap().into(),
        });
    }
    for (key, priority) in [
        ("top", RulePriority::Top),
        ("bottom", RulePriority::Bottom),
        ("external", RulePriority::Bottom),
    ] {
        for r in case[key].as_array().unwrap() {
            let query = RuleQuery::Text(r[1].as_str().unwrap().into());
            f.service
                .add_rule(&mut s, r[0].as_str().unwrap(), query, priority);
        }
    }
    (f, s)
}

/// `rewrite['slug']` when `rewrite` is an array holding a string slug.
fn rewrite_slug(rewrite: &Value) -> Option<String> {
    rewrite
        .get("slug")
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// The case's randomable post types, publicly queryable taxonomies, published posts and random content switch.
fn random_content(f: &Fixture, case: &Value) {
    *f.post_types.randomable.borrow_mut() = case["randomable"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| RandomablePostType {
            name: t["name"].as_str().unwrap().into(),
            archive_slug: t["has_archive"].as_str().map(str::to_string),
            rewrites: t["rewrite"].is_object(),
            rewrite_slug: rewrite_slug(&t["rewrite"]),
        })
        .collect();
    *f.taxonomies.0.borrow_mut() = case["taxonomies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| QueryableTaxonomy {
            name: t["name"].as_str().unwrap().into(),
            rewrite_slug: rewrite_slug(&t["rewrite"]),
            object_types: t["object_type"]
                .as_array()
                .unwrap()
                .iter()
                .map(|o| o.as_str().unwrap().to_string())
                .collect(),
        })
        .collect();
    *f.posts.0.borrow_mut() = case["posts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| (p[0].as_str().unwrap().into(), p[1].as_str().unwrap().into()))
        .collect();
    if !case["random"].as_bool().unwrap() {
        f.hooks
            .add_filter(
                &HookName::from("wp_enable_random_content_redirect"),
                Callback::rust("disable_random", |_: &[HookValue]| {
                    Ok(HookValue::Bool(false))
                }),
                &HookValue::Int(10),
                1,
            )
            .unwrap();
    }
}

fn replay(name: &str) {
    let case = cases().into_iter().find(|c| c["name"] == name).unwrap();
    let (f, mut s) = setup(&case);
    f.source.store(&s);
    let generated = f.service.rewrite_rules(&mut s, f.source.as_ref()).unwrap();
    assert_eq!(generated, rules(&case["rules"]), "rules of {name}");
    let (f, mut s) = setup(&case);
    f.source.store(&s);
    f.service.rewrite_rules(&mut s, f.source.as_ref()).unwrap();
    assert_eq!(
        f.service
            .mod_rewrite_rules(&mut s, f.source.as_ref())
            .unwrap(),
        case["mod_rewrite"].as_str().unwrap(),
        "mod_rewrite of {name}"
    );
    assert_eq!(
        f.service.iis7_url_rewrite_rules(&s, false).unwrap(),
        case["iis7"].as_str().unwrap(),
        "iis7 of {name}"
    );
    assert_eq!(
        f.service.iis7_url_rewrite_rules(&s, true).unwrap(),
        case["iis7_parent"].as_str().unwrap()
    );
}

#[test]
fn plain_permalinks_give_no_rules() {
    replay("plain");
}

#[test]
fn postname_rules_in_wordpress_order() {
    replay("postname");
}

#[test]
fn post_id_puts_posts_before_pages() {
    replay("post_id");
}

#[test]
fn date_structure_with_bases_and_page_on_front() {
    replay("date_and_bases");
}

#[test]
fn index_permalinks() {
    replay("index_permalinks");
}

#[test]
fn subdirectory_home_drops_root_files() {
    replay("subdirectory");
}

#[test]
fn extra_rules_endpoints_and_external_rules() {
    replay("extras");
}

#[test]
fn verbose_mod_rewrite_rules() {
    replay("verbose_rules");
}

#[test]
fn random_content_rules_for_authors_taxonomies_and_post_types() {
    replay("random_content");
}

#[test]
fn a_post_at_a_custom_random_base_drops_the_random_rule() {
    replay("random_base_taken");
}

#[test]
fn a_page_at_the_random_base_drops_the_random_rule() {
    replay("random_base_page_taken");
}

#[test]
fn a_post_elsewhere_leaves_the_random_rule() {
    replay("random_base_post_elsewhere");
}

#[test]
fn disabled_random_content_adds_no_rules() {
    replay("random_disabled");
}

#[test]
fn filters_fire_in_order() {
    let case = cases()
        .into_iter()
        .find(|c| c["name"] == "postname")
        .unwrap();
    let (f, mut s) = setup(&case);
    let names = [
        "post_rewrite_rules",
        "date_rewrite_rules",
        "root_rewrite_rules",
        "comments_rewrite_rules",
        "search_rewrite_rules",
        "author_rewrite_rules",
        "page_rewrite_rules",
        "category_rewrite_rules",
        "post_tag_rewrite_rules",
        "post_format_rewrite_rules",
        "wp_enable_random_content_redirect",
        "random_rewrite_rules",
        "generate_rewrite_rules",
        "rewrite_rules_array",
    ];
    for name in names {
        support::record(&f, name);
    }
    f.service.rewrite_rules(&mut s, f.source.as_ref()).unwrap();
    assert_eq!(support::hooks_run(&f), names.to_vec());
}

#[test]
fn rewrite_rules_array_filter_changes_result() {
    let case = cases()
        .into_iter()
        .find(|c| c["name"] == "postname")
        .unwrap();
    let (f, mut s) = setup(&case);
    f.hooks
        .add_filter(
            &HookName::from("rewrite_rules_array"),
            Callback::rust("only_one", |_: &[HookValue]| {
                Ok(HookValue::Array(vec![(
                    speedyspec_wp_hooks::Key::Str("x/?$".into()),
                    HookValue::Str("index.php?x=1".into()),
                )]))
            }),
            &HookValue::Int(10),
            1,
        )
        .unwrap();
    let generated = f.service.rewrite_rules(&mut s, f.source.as_ref()).unwrap();
    assert_eq!(
        generated,
        vec![Rule {
            pattern: "x/?$".into(),
            query: "index.php?x=1".into()
        }]
    );
    assert_eq!(s.rules, Some(generated));
}

#[test]
fn generate_rewrite_rules_action_can_mutate() {
    let case = cases()
        .into_iter()
        .find(|c| c["name"] == "postname")
        .unwrap();
    let (f, mut s) = setup(&case);
    let cell = f.source.cell();
    f.hooks
        .add_filter(
            &HookName::from("generate_rewrite_rules"),
            Callback::rust("plugin", move |_: &[HookValue]| {
                let mut state = cell.borrow_mut();
                let mut rules = state.rules.clone().unwrap_or_default();
                rules.insert(
                    0,
                    Rule {
                        pattern: "plugin/?$".into(),
                        query: "index.php?plugin=1".into(),
                    },
                );
                state.rules = Some(rules);
                Ok(HookValue::Null)
            }),
            &HookValue::Int(10),
            1,
        )
        .unwrap();
    let generated = f.service.rewrite_rules(&mut s, f.source.as_ref()).unwrap();
    assert_eq!(generated[0].pattern, "plugin/?$");
}
