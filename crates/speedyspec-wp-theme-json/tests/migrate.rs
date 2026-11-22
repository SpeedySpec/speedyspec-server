//! `WP_Theme_JSON_Schema::migrate`, with WordPress's two tests and the edges of its `switch`.

mod support;

use speedyspec_hooks::Value;
use speedyspec_wp_theme_json::migrate;
use speedyspec_wp_theme_json_contracts::Origin;
use support::{assert_php_eq, json};

#[test]
fn migrate_v1_to_latest() {
    let v1 = json(r##"{"version":1,"settings":{"color":{"palette":[{"name":"Pale Pink","slug":"pale-pink","color":"#f78da7"},{"name":"Vivid Red","slug":"vivid-red","color":"#cf2e2e"}],"custom":false,"link":true},"border":{"color":false,"customRadius":false,"style":false,"width":false},"typography":{"fontSizes":[{"name":"Small","slug":"small","size":12},{"name":"Normal","slug":"normal","size":16}],"fontStyle":false,"fontWeight":false,"letterSpacing":false,"textDecoration":false,"textTransform":false},"blocks":{"core/group":{"border":{"color":true,"customRadius":true,"style":true,"width":true},"typography":{"fontStyle":true,"fontWeight":true,"letterSpacing":true,"textDecoration":true,"textTransform":true}}}},"styles":{"color":{"background":"purple"},"blocks":{"core/group":{"color":{"background":"red"},"spacing":{"padding":{"top":"10px"}},"elements":{"link":{"color":{"text":"yellow"}}}}},"elements":{"link":{"color":{"text":"red"}}}}}"##);
    let expected = json(r##"{"version":3,"settings":{"color":{"palette":[{"name":"Pale Pink","slug":"pale-pink","color":"#f78da7"},{"name":"Vivid Red","slug":"vivid-red","color":"#cf2e2e"}],"custom":false,"link":true},"border":{"color":false,"radius":false,"style":false,"width":false},"typography":{"defaultFontSizes":false,"fontSizes":[{"name":"Small","slug":"small","size":12},{"name":"Normal","slug":"normal","size":16}],"fontStyle":false,"fontWeight":false,"letterSpacing":false,"textDecoration":false,"textTransform":false},"blocks":{"core/group":{"border":{"color":true,"radius":true,"style":true,"width":true},"typography":{"fontStyle":true,"fontWeight":true,"letterSpacing":true,"textDecoration":true,"textTransform":true}}}},"styles":{"color":{"background":"purple"},"blocks":{"core/group":{"color":{"background":"red"},"spacing":{"padding":{"top":"10px"}},"elements":{"link":{"color":{"text":"yellow"}}}}},"elements":{"link":{"color":{"text":"red"}}}}}"##);
    assert_php_eq(&migrate(v1, Origin::Theme), &expected);
}

#[test]
fn migrate_v2_to_latest() {
    let v2 = json(r##"{"version":2,"settings":{"typography":{"fontSizes":[{"name":"Small","slug":"small","size":12},{"name":"Normal","slug":"normal","size":16}]},"spacing":{"spacingSizes":[{"name":"Small","slug":20,"size":"20px"},{"name":"Large","slug":80,"size":"80px"}]}}}"##);
    let expected = json(r##"{"version":3,"settings":{"typography":{"defaultFontSizes":false,"fontSizes":[{"name":"Small","slug":"small","size":12},{"name":"Normal","slug":"normal","size":16}]},"spacing":{"defaultSpacingSizes":false,"spacingSizes":[{"name":"Small","slug":20,"size":"20px"},{"name":"Large","slug":80,"size":"80px"}]}}}"##);
    assert_php_eq(&migrate(v2, Origin::Theme), &expected);
}

#[test]
fn spacing_scale_is_dropped_when_sizes_exist() {
    let v2 = json(r##"{"version":2,"settings":{"spacing":{"spacingSizes":[],"spacingScale":{"steps":3}}}}"##);
    let expected = json(r##"{"version":3,"settings":{"spacing":{"spacingSizes":[],"defaultSpacingSizes":false}}}"##);
    assert_php_eq(&migrate(v2, Origin::Theme), &expected);
}

#[test]
fn custom_origin_only_bumps_the_version() {
    let v2 = json(r##"{"version":2,"settings":{"typography":{"fontSizes":[]}}}"##);
    let expected = json(r##"{"version":3,"settings":{"typography":{"fontSizes":[]}}}"##);
    assert_php_eq(&migrate(v2, Origin::Custom), &expected);
}

#[test]
fn missing_version_discards_content() {
    let doc = json(r##"{"settings":{"color":{"custom":false}}}"##);
    assert_php_eq(&migrate(doc, Origin::Theme), &json(r##"{"version":3}"##));
    assert_php_eq(&migrate(json(r##"{"version":null}"##), Origin::Theme), &json(r##"{"version":3}"##));
    assert_php_eq(&migrate(Value::Str("x".into()), Origin::Theme), &json(r##"{"version":3}"##));
}

#[test]
fn string_version_one_migrates() {
    let doc = json(r##"{"version":"1","settings":{"border":{"customRadius":true}}}"##);
    assert_php_eq(&migrate(doc, Origin::Theme), &json(r##"{"version":3,"settings":{"border":{"radius":true}}}"##));
}

#[test]
fn version_four_passes_through() {
    let doc = json(r##"{"version":4,"settings":{"border":{"customRadius":true}}}"##);
    assert_php_eq(&migrate(doc.clone(), Origin::Theme), &doc);
}

#[test]
fn null_renamed_value_is_not_moved() {
    let doc = json(r##"{"version":1,"settings":{"spacing":{"customMargin":null}}}"##);
    assert_php_eq(&migrate(doc, Origin::Theme), &json(r##"{"version":3,"settings":{"spacing":{"customMargin":null}}}"##));
}

#[test]
fn true_version_migrates_as_one() {
    let doc = json(r##"{"version":true,"settings":{"border":{"customRadius":true}}}"##);
    assert_php_eq(&migrate(doc, Origin::Theme), &json(r##"{"version":3,"settings":{"border":{"radius":true}}}"##));
}
