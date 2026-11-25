<?php
/**
 * Records WordPress's own WP_Rewrite::generate_rewrite_rules() output for the permastruct expansion tests.
 *
 * Run with the path to an unpacked WordPress release (the rewrite area's pinned release, as
 * `speedyspec_compat::wordpress_release()` unpacks it), writing cases.json beside this file:
 *
 *     php record.php /path/to/wordpress > cases.json
 *
 * Only class-wp-rewrite.php is loaded. The three WordPress functions generate_rewrite_rules() calls are stubbed with
 * the values each case sets: get_option( 'page_on_front' ), get_post_types() and is_post_type_hierarchical().
 */

$root = rtrim( $argv[1] ?? '', '/' );
if ( ! is_file( "$root/wp-includes/class-wp-rewrite.php" ) ) {
	fwrite( STDERR, "usage: php record.php /path/to/wordpress\n" );
	exit( 1 );
}

foreach ( array(
	'EP_NONE' => 0, 'EP_PERMALINK' => 1, 'EP_ATTACHMENT' => 2, 'EP_DATE' => 4, 'EP_YEAR' => 8, 'EP_MONTH' => 16,
	'EP_DAY' => 32, 'EP_ROOT' => 64, 'EP_COMMENTS' => 128, 'EP_SEARCH' => 256, 'EP_CATEGORIES' => 512,
	'EP_TAGS' => 1024, 'EP_AUTHORS' => 2048, 'EP_PAGES' => 4096, 'EP_ALL_ARCHIVES' => 3644, 'EP_ALL' => 8191,
) as $name => $value ) {
	define( $name, $value );
}

$GLOBALS['case'] = array();

function get_option( $name ) {
	return 'page_on_front' === $name ? ( $GLOBALS['case']['page_on_front'] ?? '' ) : false;
}

function get_post_types( $args = array() ) {
	$types = array_keys( $GLOBALS['case']['post_types'] ?? array() );
	return array_combine( $types, $types );
}

function is_post_type_hierarchical( $type ) {
	return (bool) ( $GLOBALS['case']['post_types'][ $type ] ?? false );
}

require "$root/wp-includes/class-wp-rewrite.php";

$cases = array(
	array( 'name' => 'postname_structure', 'struct' => '/%postname%/', 'ep_mask' => 1 ),
	array( 'name' => 'date_structure', 'struct' => '/%year%/%monthnum%/%day%/', 'ep_mask' => 4 ),
	array(
		'name'      => 'category_with_endpoints',
		'struct'    => '/category/%category%',
		'ep_mask'   => 512,
		'endpoints' => array( array( 8191, 'json', 'json' ), array( 2, 'print', 'print' ) ),
	),
	array( 'name' => 'forcomments', 'struct' => '/comments/', 'ep_mask' => 128, 'paged' => false, 'forcomments' => true ),
	array( 'name' => 'walk_dirs_false', 'struct' => '/%year%/%monthnum%/%postname%/', 'ep_mask' => 1, 'walk_dirs' => false ),
	array( 'name' => 'index_permalinks', 'struct' => '/index.php/%postname%/', 'ep_mask' => 1 ),
	array( 'name' => 'no_trailing_slash', 'struct' => '/%postname%', 'ep_mask' => 1 ),
	array( 'name' => 'root_with_page_on_front', 'struct' => '/', 'ep_mask' => 64, 'page_on_front' => '7' ),
	array( 'name' => 'pagename', 'struct' => '%pagename%', 'ep_mask' => 4096 ),
	array(
		'name'       => 'attachment_endpoint_on_post',
		'struct'     => '/%year%/%monthnum%/%day%/%postname%/',
		'ep_mask'    => 1,
		'endpoints'  => array( array( 3, 'amp', 'amp' ) ),
	),
	array(
		'name'       => 'custom_post_type',
		'struct'     => '/book/%book%',
		'ep_mask'    => 0,
		'post_types' => array( 'book' => false ),
		'tags'       => array( array( '%book%', '([^/]+)', 'book=' ) ),
	),
	array(
		'name'       => 'hierarchical_custom_post_type',
		'struct'     => '/guide/%guide%',
		'ep_mask'    => 0,
		'post_types' => array( 'guide' => true ),
		'tags'       => array( array( '%guide%', '(.+?)', 'guide=' ) ),
	),
	array( 'name' => 'generate_rewrite_rule', 'struct' => '/author/%author%', 'rule' => true ),
	array( 'name' => 'no_tags', 'struct' => '/plain/path', 'ep_mask' => 0 ),
	array( 'name' => 'plain_matches', 'struct' => '/%postname%/', 'ep_mask' => 1, 'matches' => '' ),
);

$out = array();
foreach ( $cases as $case ) {
	$GLOBALS['case'] = $case;
	$rewrite = ( new ReflectionClass( 'WP_Rewrite' ) )->newInstanceWithoutConstructor();
	$rewrite->index     = 'index.php';
	$rewrite->matches   = $case['matches'] ?? 'matches';
	$rewrite->endpoints = $case['endpoints'] ?? array();
	foreach ( $case['tags'] ?? array() as $tag ) {
		$rewrite->add_rewrite_tag( $tag[0], $tag[1], $tag[2] );
	}
	if ( ! empty( $case['rule'] ) ) {
		$rules = $rewrite->generate_rewrite_rule( $case['struct'] );
	} else {
		$rules = $rewrite->generate_rewrite_rules(
			$case['struct'],
			$case['ep_mask'],
			$case['paged'] ?? true,
			$case['feed'] ?? true,
			$case['forcomments'] ?? false,
			$case['walk_dirs'] ?? true,
			$case['endpoints_on'] ?? true
		);
	}
	$pairs = array();
	foreach ( $rules as $regex => $query ) {
		$pairs[] = array( (string) $regex, $query );
	}
	$tags = array();
	foreach ( $rewrite->rewritecode as $i => $code ) {
		$tags[] = array( $code, $rewrite->rewritereplace[ $i ], $rewrite->queryreplace[ $i ] );
	}
	$out[] = array(
		'name'          => $case['name'],
		'struct'        => $case['struct'],
		'rule'          => ! empty( $case['rule'] ),
		'ep_mask'       => $case['ep_mask'] ?? 0,
		'paged'         => $case['paged'] ?? true,
		'feed'          => $case['feed'] ?? true,
		'forcomments'   => $case['forcomments'] ?? false,
		'walk_dirs'     => $case['walk_dirs'] ?? true,
		'matches'       => $rewrite->matches,
		'page_on_front' => $case['page_on_front'] ?? '',
		'post_types'    => $case['post_types'] ?? new stdClass(),
		'endpoints'     => $rewrite->endpoints,
		'tags'          => $tags,
		'feeds'         => $rewrite->feeds,
		'rules'         => $pairs,
	);
}

echo json_encode( $out, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE ), "\n";
