//! Every worker declares the text, content and transport ports: WordPress never runs in the runtime, so each port's
//! functions and classes are the only declarations of their names.

mod support;

/// Where each name is declared (`native` for a native function or class, the `speedyspec://` source of a facade),
/// and what a few calls only the ports can answer return.
const PROBE: &str = r#"<?php
$where = function ( $name ) {
    $r = function_exists( $name ) ? new ReflectionFunction( $name )
        : ( class_exists( $name ) ? new ReflectionClass( $name ) : null );
    return null === $r ? 'missing' : ( $r->isInternal() ? 'native' : (string) $r->getFileName() );
};
$names = [
    'wp_is_valid_utf8', 'WP_Token_Map', '__', '_x', 'wp_kses', 'safecss_filter_attr', 'wpautop', 'wptexturize',
    'do_shortcode', 'WP_Theme_JSON', 'WP_Theme_JSON_Resolver', 'WP_Rewrite', 'add_rewrite_rule',
    'SpeedySpec\Operation\Operations', 'SpeedySpec\Abilities\Abilities', 'WP_Ability', 'SpeedySpec\Rest\Rest',
    'WP_REST_Server', 'SpeedySpec\XmlRpc\XmlRpc',
];
add_shortcode( 'hi', function () { return 'hello'; } );
echo json_encode( [
    'where'     => array_combine( $names, array_map( $where, $names ) ),
    'utf8'      => [ wp_is_valid_utf8( "\xff" ), wp_is_valid_utf8( 'é' ) ],
    'map'       => WP_Token_Map::from_array( [ 'amp' => '&' ] )->contains( 'amp' ),
    'kses'      => wp_kses( '<b>x</b><script>y</script>', [ 'b' => [] ] ),
    'autop'     => wpautop( "a\n\nb" ),
    'shortcode' => do_shortcode( '[hi]' ),
] );
"#;

#[tokio::test(flavor = "current_thread")]
async fn every_worker_declares_the_ports() {
    let dir = tempfile::tempdir().unwrap();
    let app = support::release(dir.path(), "app", PROBE);
    let mut worker = support::spawn(dir.path(), &app, None, &[]);
    worker.ready().await;
    let answer = worker.get(&app.join("index.php"), "/").await;
    assert_eq!(answer.status(), 200, "{}", answer.stdout);
    let got: serde_json::Value = serde_json::from_str(answer.body()).expect(&answer.stdout);

    for (name, declared) in got["where"].as_object().unwrap() {
        let declared = declared.as_str().unwrap();
        assert!(
            declared == "native" || declared.starts_with("speedyspec://"),
            "{name} is declared by {declared}, not a port"
        );
    }
    assert_eq!(got["utf8"], serde_json::json!([false, true]));
    assert_eq!(got["map"], serde_json::json!(true));
    assert_eq!(got["kses"], "<b>x</b>y");
    assert_eq!(got["autop"], "<p>a</p>\n<p>b</p>\n");
    assert_eq!(got["shortcode"], "hello");
}

/// The cron port's schedules translate their names through `__()`: with nothing loaded for the domain, the i18n port
/// answers the text itself (its uncommon path reaches the fixtures' `get_translations_for_domain()`).
#[tokio::test(flavor = "current_thread")]
async fn translations_answer_with_nothing_loaded() {
    let dir = tempfile::tempdir().unwrap();
    let stubs =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wordpress-stubs.php");
    let app = support::release(
        dir.path(),
        "app",
        &format!(
            "<?php require {:?}; echo json_encode([__('Once Hourly'), _x('Post', 'noun'), _n('one', 'many', 2)]);",
            stubs.display().to_string()
        ),
    );
    let mut worker = support::spawn(dir.path(), &app, None, &[]);
    worker.ready().await;
    let answer = worker.get(&app.join("index.php"), "/").await;
    assert_eq!(
        answer.body(),
        r#"["Once Hourly","Post","many"]"#,
        "{}",
        answer.stdout
    );
}
