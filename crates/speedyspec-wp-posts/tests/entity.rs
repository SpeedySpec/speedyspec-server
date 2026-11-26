//! Replays what `compat/posts-entity/record.php` recorded from WordPress 7.2-RC2: `wp_resolve_post_date()` with no
//! `wp_checkdate` callback, its clock replayed from the recording, and the `get_gmt_from_date()` of each date it
//! returned, which `wp_insert_post()` takes as the GMT date of a post with no GMT date and a status that does not
//! float.

mod support;

use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_posts::{
    InsertDateInput, PostStatusRegistry, insert_post_dates, resolve_post_date,
};
use support::entity::{TableClock, entity_cases};

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
