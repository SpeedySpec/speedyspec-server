//! MySQL's loose GROUP BY: columns neither grouped nor dependent on a grouped primary key read any row's value.

mod common;

use common::{assert_pg, wp_dialect};

fn same(mysql: &str, pg: &str) {
    assert_pg(&wp_dialect(), mysql, vec![], &[pg]);
}

#[test]
fn wraps_ungrouped() {
    same(
        "SELECT post_type, post_status, COUNT(*) AS n FROM wp_posts GROUP BY post_type",
        r#"SELECT "post_type", any_value("post_status") AS "post_status", COUNT(*) AS "n" FROM "wp_posts" GROUP BY "post_type""#,
    );
}

#[test]
fn primary_key_dependency_untouched() {
    same(
        "SELECT wp_posts.* FROM wp_posts INNER JOIN wp_postmeta ON (wp_posts.ID = wp_postmeta.post_id) GROUP BY wp_posts.ID",
        r#"SELECT "wp_posts".* FROM "wp_posts" INNER JOIN "wp_postmeta" ON ("wp_posts"."ID" = "wp_postmeta"."post_id") GROUP BY "wp_posts"."ID""#,
    );
}

#[test]
fn aggregate_without_group_by() {
    same(
        "SELECT p.post_title, COUNT(*) FROM wp_posts p",
        r#"SELECT any_value("p"."post_title") AS "post_title", COUNT(*) AS "COUNT(*)" FROM "wp_posts" AS "p""#,
    );
}

#[test]
fn star_expanded() {
    same(
        "SELECT * FROM wp_postmeta GROUP BY meta_key",
        r#"SELECT any_value("meta_id") AS "meta_id", any_value("post_id") AS "post_id", "meta_key", any_value("meta_value") AS "meta_value" FROM "wp_postmeta" GROUP BY "meta_key""#,
    );
}

#[test]
fn having_alias_and_order_by() {
    same(
        "SELECT meta_key, COUNT(*) AS c FROM wp_postmeta GROUP BY meta_key HAVING c > 1 ORDER BY post_id",
        r#"SELECT "meta_key", COUNT(*) AS "c" FROM "wp_postmeta" GROUP BY "meta_key" HAVING COUNT(*) > 1 ORDER BY any_value("post_id")"#,
    );
}

#[test]
fn distinct_ordered_by_an_unselected_column() {
    same(
        "SELECT DISTINCT(wp_posts.ID) FROM wp_posts WHERE 1=1 ORDER BY post_title LIMIT 0, 50",
        r#"SELECT ("wp_posts"."ID") AS "(wp_posts.ID)" FROM "wp_posts" WHERE 1 = 1 GROUP BY "wp_posts"."ID" ORDER BY "post_title" LIMIT 50 OFFSET 0"#,
    );
}

#[test]
fn distinct_with_an_aggregate_groups_the_rest() {
    same(
        "SELECT DISTINCT post_type, COUNT(ID) AS n FROM wp_posts GROUP BY post_type ORDER BY post_title",
        r#"SELECT "post_type", COUNT("ID") AS "n" FROM "wp_posts" GROUP BY "post_type" ORDER BY any_value("post_title")"#,
    );
    same(
        "SELECT DISTINCT p.ID FROM wp_posts p JOIN wp_postmeta m ON m.post_id = p.ID ORDER BY m.meta_value + 0",
        r#"SELECT "p"."ID" FROM "wp_posts" AS "p" JOIN "wp_postmeta" AS "m" ON "m"."post_id" = "p"."ID" GROUP BY "p"."ID" ORDER BY speedyspec_double(any_value("m"."meta_value")) + 0"#,
    );
}
