//! Replays what `compat/posts-entity/record.php` recorded from WordPress 7.2-RC2: `wp_resolve_post_date()` with no
//! `wp_checkdate` callback, its clock replayed from the recording, and the `get_gmt_from_date()` of each date it
//! returned, which `wp_insert_post()` takes as the GMT date of a post with no GMT date and a status that does not
//! float; `wp_unique_post_slug()` with no filters, its result and its `$wpdb` statements in order; the hierarchy
//! functions (`wp_get_post_parent_id()`, `get_post_ancestors()`, `get_page_uri()` before its filter,
//! `wp_check_post_hierarchy_for_loops()` and `wp_find_hierarchy_loop()`) with no object cache, their results and
//! statements; and `get_page_hierarchy()` and `get_page_children()` over lists of posts, with no database.

mod support;

use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_posts::{
    InsertDateInput, NoBadSlugs, PostStatusRegistry, SlugPorts, SlugRequest, Uncached,
    check_post_hierarchy_for_loops, extended, find_hierarchy_loop, insert_post_dates,
    page_children, page_hierarchy, page_uri, post_ancestors, post_parent_id, resolve_post_date,
    unique_post_slug,
};
use speedyspec_wp_posts_contracts::{PostId, PostParent, PostReader};
use support::entity::{
    FakeTypes, Recording, TableClock, TextTruncation, context, entity_cases, row_post, seed,
    statements,
};

fn statuses() -> PostStatusRegistry {
    let mut statuses = PostStatusRegistry::new();
    statuses.register(
        b"draft",
        vec![(PhpKey::Str(b"date_floating".to_vec()), PhpValue::Bool(true))],
    );
    statuses.register(b"private", Vec::new());
    statuses
}

#[test]
fn dates_match_wordpress() {
    let cases = entity_cases();
    let dates = cases["dates"].as_array().unwrap();
    assert_eq!(dates.len(), 15);
    let statuses = statuses();
    for case in dates {
        let name = case["name"].as_str().unwrap();
        let bytes = |field: &str| match support::decode(&case[field]) {
            PhpValue::Str(s) => s,
            other => panic!("{name}: {field} is a string, not {other:?}"),
        };
        let clock = TableClock::from_case(&case["clock"]);
        let resolved = resolve_post_date(&bytes("post_date"), &bytes("post_date_gmt"), &clock);
        let expected = match support::decode(&case["result"]) {
            PhpValue::Str(s) => Some(s),
            PhpValue::Bool(false) => None,
            other => panic!("{name}: unrecorded result {other:?}"),
        };
        assert_eq!(resolved.unfiltered(), expected, "{name}");

        // The GMT date wp_insert_post() derives from the resolved date when it is given none: the recorded
        // get_gmt_from_date() for a status that does not float, zero for one that does.
        if let Some(date) = &expected {
            let insert = |status| {
                insert_post_dates(
                    &InsertDateInput {
                        post_date: date,
                        post_date_gmt: b"",
                        status,
                        post_type: "attachment",
                        update: false,
                    },
                    &statuses,
                    &clock,
                )
            };
            let (_, gmt) = &clock.gmt_from_date_pairs()[0];
            assert_eq!(&insert("private").date_gmt, gmt, "{name}: date_gmt");
            assert_eq!(
                insert("draft").date_gmt,
                b"0000-00-00 00:00:00",
                "{name}: floating date_gmt"
            );
        }
    }
}

#[test]
#[should_panic(expected = "recorded no now")]
fn clock_refuses_now_where_none_was_recorded() {
    use speedyspec_wp_posts_contracts::Clock;
    let cases = entity_cases();
    let leap_day = cases["dates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["name"] == "leap-day")
        .unwrap();
    TableClock::from_case(&leap_day["clock"]).now_local();
}

#[test]
fn slug_scenarios_match_wordpress() {
    let cases = entity_cases();
    let scenarios = cases["slugs"].as_array().unwrap();
    assert_eq!(scenarios.len(), 8);
    let mut calls = 0;
    for scenario in scenarios {
        let name = scenario["name"].as_str().unwrap();
        let repository = Recording::new(seed(&scenario["rows"]));
        let types = FakeTypes::from_scenario(&scenario["post_types"]);
        let context = context(scenario["permalinks"].as_str().unwrap());
        let reader = Uncached(&repository);
        let ports = SlugPorts {
            repository: &repository,
            posts: &reader,
            types: &types,
            truncation: &TextTruncation,
            bad: &NoBadSlugs,
        };
        for call in scenario["calls"].as_array().unwrap() {
            calls += 1;
            assert_eq!(call["fn"], "wp_unique_post_slug", "{name}");
            let PhpValue::Array(args) = support::decode(&call["args"]) else {
                panic!("{name}: the arguments are an array");
            };
            let arg = |i: usize| &args[i].1;
            let bytes = |i: usize| match arg(i) {
                PhpValue::Str(s) => s.clone(),
                other => panic!("{name}: argument {i} is a string, not {other:?}"),
            };
            let id = |i: usize| match arg(i) {
                PhpValue::Int(n) => PostId(u64::try_from(*n).unwrap()),
                other => panic!("{name}: argument {i} is an int, not {other:?}"),
            };
            let (slug, status, post_type) = (bytes(0), bytes(2), bytes(3));
            let status = String::from_utf8_lossy(&status);
            let post_type = String::from_utf8_lossy(&post_type);
            let request = SlugRequest {
                slug: &slug,
                post_id: id(1),
                status: &status,
                post_type: &post_type,
                parent: PostParent::from(id(4)),
            };
            let label = format!("{name}: {:?}", String::from_utf8_lossy(&slug));
            // The skip scenario proves the no-check guard runs before any query: its calls record no statements.
            let result = unique_post_slug(&request, &context, &ports).unwrap();
            assert_eq!(
                PhpValue::Str(result),
                support::decode(&call["result"]),
                "{label}"
            );
            assert_eq!(
                repository.take(),
                statements(&call["statements"]),
                "{label}: statements"
            );
        }
    }
    assert_eq!(calls, 40);
}

/// A recorded call's arguments, decoded.
fn call_args(call: &serde_json::Value) -> Vec<PhpValue> {
    let PhpValue::Array(args) = support::decode(&call["args"]) else {
        panic!("the arguments are an array");
    };
    args.into_iter().map(|(_, v)| v).collect()
}

/// An int argument as a post ID.
fn id_of(value: &PhpValue) -> PostId {
    match value {
        PhpValue::Int(n) => PostId(u64::try_from(*n).unwrap()),
        other => panic!("an ID is an int, not {other:?}"),
    }
}

/// A PHP list of IDs (`array( 0 => 4, 1 => 3 )`).
fn id_list(ids: &[PostId]) -> PhpValue {
    PhpValue::Array(
        ids.iter()
            .enumerate()
            .map(|(i, id)| {
                (
                    PhpKey::Int(i64::try_from(i).unwrap()),
                    PhpValue::Int(i64::try_from(id.0).unwrap()),
                )
            })
            .collect(),
    )
}

/// An ID, or false for `None`.
fn id_or_false(id: Option<PostId>) -> PhpValue {
    id.map_or(PhpValue::Bool(false), |id| {
        PhpValue::Int(i64::try_from(id.0).unwrap())
    })
}

#[test]
fn hierarchy_matches_wordpress() {
    let cases = entity_cases();
    let scenarios = cases["hierarchy"].as_array().unwrap();
    assert_eq!(scenarios.len(), 1);
    let mut calls = 0;
    for scenario in scenarios {
        let repository = Recording::new(seed(&scenario["rows"]));
        let reader = Uncached(&repository);
        for call in scenario["calls"].as_array().unwrap() {
            calls += 1;
            let function = call["fn"].as_str().unwrap();
            let args = call_args(call);
            let label = format!("{function}{args:?}");
            let result = match function {
                "wp_get_post_parent_id" => {
                    id_or_false(post_parent_id(&reader, id_of(&args[0])).unwrap())
                }
                "get_post_ancestors" => match reader.post(id_of(&args[0])).unwrap() {
                    Some(post) => id_list(&post_ancestors(&reader, &post).unwrap()),
                    None => id_list(&[]),
                },
                "get_page_uri" => match reader.post(id_of(&args[0])).unwrap() {
                    Some(page) => PhpValue::Str(page_uri(&reader, &page).unwrap()),
                    None => PhpValue::Bool(false),
                },
                "wp_check_post_hierarchy_for_loops" => {
                    let check =
                        check_post_hierarchy_for_loops(id_of(&args[0]), id_of(&args[1]), &reader)
                            .unwrap();
                    // No recorded call meets a loop without the post: WordPress would have updated the table.
                    assert!(check.break_loop.is_empty(), "{label}: break_loop");
                    PhpValue::Int(i64::try_from(check.parent.0).unwrap())
                }
                "wp_find_hierarchy_loop" => {
                    assert_eq!(args[0], PhpValue::Str(b"wp_get_post_parent_id".to_vec()));
                    let start_parent = match &args[2] {
                        PhpValue::Null => None,
                        parent => Some(id_of(parent)),
                    };
                    let members =
                        find_hierarchy_loop(&reader, id_of(&args[1]), start_parent).unwrap();
                    PhpValue::Array(
                        members
                            .iter()
                            .map(|id| {
                                (
                                    PhpKey::Int(i64::try_from(id.0).unwrap()),
                                    PhpValue::Bool(true),
                                )
                            })
                            .collect(),
                    )
                }
                other => panic!("unrecorded function {other}"),
            };
            assert_eq!(result, support::decode(&call["result"]), "{label}");
            assert_eq!(
                repository.take(),
                statements(&call["statements"]),
                "{label}: statements"
            );
        }
    }
    assert_eq!(calls, 21);
}

#[test]
fn pages_match_wordpress() {
    let cases = entity_cases();
    let entries = cases["pages"].as_array().unwrap();
    assert_eq!(entries.len(), 4);
    let mut calls = 0;
    for entry in entries {
        let name = entry["name"].as_str().unwrap();
        let pages: Vec<_> = entry["pages"]
            .as_array()
            .unwrap()
            .iter()
            .map(row_post)
            .collect();
        for call in entry["calls"].as_array().unwrap() {
            calls += 1;
            let function = call["fn"].as_str().unwrap();
            let page_id = id_of(&call_args(call)[0]);
            let label = format!("{name}: {function}({})", page_id.0);
            match function {
                "get_page_hierarchy" => {
                    let result = PhpValue::Array(
                        page_hierarchy(&pages, page_id)
                            .into_iter()
                            .map(|(id, slug)| {
                                (
                                    PhpKey::Int(i64::try_from(id.0).unwrap()),
                                    PhpValue::Str(slug.as_bytes().to_vec()),
                                )
                            })
                            .collect(),
                    );
                    assert_eq!(result, support::decode(&call["result"]), "{label}");
                }
                "get_page_children" => {
                    let expected: Vec<_> = call["result"]["v"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|pair| row_post(&pair[1]))
                        .collect();
                    assert_eq!(page_children(page_id, &pages), expected, "{label}");
                }
                other => panic!("{name}: unrecorded function {other}"),
            }
        }
    }
    assert_eq!(calls, 16);
}

#[test]
fn extended_matches_wordpress() {
    let cases = entity_cases();
    let entries = cases["extended"].as_array().unwrap();
    assert_eq!(entries.len(), 10);
    for (index, entry) in entries.iter().enumerate() {
        let PhpValue::Str(content) = support::decode(&entry["content"]) else {
            panic!("case {index}: content is not a string");
        };
        let parts = extended(&content);
        let result = PhpValue::Array(vec![
            (PhpKey::Str(b"main".to_vec()), PhpValue::Str(parts.main)),
            (
                PhpKey::Str(b"extended".to_vec()),
                PhpValue::Str(parts.extended),
            ),
            (
                PhpKey::Str(b"more_text".to_vec()),
                PhpValue::Str(parts.more_text),
            ),
        ]);
        assert_eq!(result, support::decode(&entry["result"]), "case {index}");
    }
}
