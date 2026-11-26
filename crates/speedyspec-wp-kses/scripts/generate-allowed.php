<?php
// Writes data/allowed-<release>.json: the pinned WordPress release's $allowedposttags (after
// _wp_add_global_attributes) and $allowedtags, as nested [key, value] pairs so key order survives JSON.
// Usage: php scripts/generate-allowed.php <wordpress-root> > data/allowed-<release>.json
$wp = rtrim( $argv[1], '/' );
define( 'ABSPATH', $wp . '/' );
function apply_filters( $hook, $value ) { return $value; }
function add_filter() {}
function add_action() {}
require "$wp/wp-includes/kses.php";
$pairs = static function ( $v ) use ( &$pairs ) {
	if ( ! is_array( $v ) ) {
		return $v;
	}
	$out = array();
	foreach ( $v as $k => $item ) {
		$out[] = array( $k, $pairs( $item ) );
	}
	return array( 'pairs' => $out );
};
echo json_encode( array( 'allowedposttags' => $pairs( $allowedposttags ), 'allowedtags' => $pairs( $allowedtags ) ), JSON_UNESCAPED_SLASHES ), "\n";
