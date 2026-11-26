<?php
/**
 * Records WordPress's own WP_Rewrite::rewrite_rules(), mod_rewrite_rules() and iis7_url_rewrite_rules() output for
 * the generation tests.
 *
 *     php record.php /path/to/wordpress > cases.json
 *
 * Only class-wp-rewrite.php is loaded. Each case sets the options, URLs and registrations WordPress would have at
 * that point (the category, tag and post format permastructs register_taxonomy() adds), and the WordPress functions
 * the class calls are stubbed: filters return their value unchanged, so this records generation alone. A case may
 * also list the randomable post types, the publicly queryable taxonomies (with the permastructs register_taxonomy()
 * would add for them), the posts published at a path, and whether random content redirects are on.
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
define( 'OBJECT', 'OBJECT' );

$GLOBALS['case'] = array();

function get_option( $name ) {
	return $GLOBALS['case']['options'][ $name ] ?? false;
}
function home_url() {
	return $GLOBALS['case']['home'];
}
function site_url() {
	return $GLOBALS['case']['site'];
}
function is_multisite() {
	return false;
}
function is_main_site() {
	return true;
}
function get_post_types( $args = array(), $output = 'names' ) {
	if ( empty( $args['randomable'] ) ) {
		return array();
	}
	$types = array();
	foreach ( $GLOBALS['case']['randomable'] ?? array() as $type ) {
		$types[ $type['name'] ] = 'objects' === $output ? (object) $type : $type['name'];
	}
	return $types;
}
function get_taxonomies( $args = array(), $output = 'names' ) {
	$taxonomies = array();
	foreach ( $GLOBALS['case']['taxonomies'] ?? array() as $taxonomy ) {
		$taxonomies[ $taxonomy['name'] ] = (object) $taxonomy;
	}
	return $taxonomies;
}
final class WP_Post {
	public $post_type;
}
function get_page_by_path( $path, $output = OBJECT, $post_type = 'page' ) {
	foreach ( $GLOBALS['case']['posts'] ?? array() as $post ) {
		if ( array( $path, $post_type ) === $post ) {
			$found            = new WP_Post();
			$found->post_type = $post_type;
			return $found;
		}
	}
	return null;
}
function is_post_publicly_viewable( $post ) {
	return true;
}
function wp_is_random_content_redirect_enabled() {
	return $GLOBALS['case']['random'] ?? true;
}
function get_locale() {
	return 'en_US';
}
function switch_to_locale( $locale ) {
	return false;
}
function restore_previous_locale() {
	return false;
}
function _x( $text, $context ) {
	return $text;
}
function is_post_type_hierarchical( $type ) {
	return false;
}
function apply_filters( $hook, $value ) {
	return $value;
}
function apply_filters_deprecated( $hook, $args ) {
	return $args[0];
}
function do_action_ref_array( $hook, $args ) {
}
function did_action( $hook ) {
	return 1;
}
function trailingslashit( $value ) {
	return rtrim( $value, '/\\' ) . '/';
}
function esc_attr( $text ) {
	return htmlspecialchars( $text, ENT_QUOTES, 'UTF-8', false );
}
function wp_parse_args( $args, $defaults ) {
	return array_merge( $defaults, $args );
}

require "$root/wp-includes/class-wp-rewrite.php";

function taxonomies( WP_Rewrite $rewrite, array $options ) {
	$rewrite->add_rewrite_tag( '%category%', '(.+?)', 'category_name=' );
	$rewrite->add_rewrite_tag( '%post_tag%', '([^/]+)', 'tag=' );
	$rewrite->add_rewrite_tag( '%post_format%', '([^/]+)', 'post_format=' );
	$category = ( $options['category_base'] ?? '' ) ?: 'category';
	$tag      = ( $options['tag_base'] ?? '' ) ?: 'tag';
	$rewrite->add_permastruct( 'category', "$category/%category%", array( 'with_front' => true, 'ep_mask' => EP_CATEGORIES ) );
	$rewrite->add_permastruct( 'post_tag', "$tag/%post_tag%", array( 'with_front' => true, 'ep_mask' => EP_TAGS ) );
	$rewrite->add_permastruct( 'post_format', 'type/%post_format%', array( 'with_front' => true ) );
}

$cases = array(
	array( 'name' => 'plain', 'options' => array( 'permalink_structure' => '' ) ),
	array( 'name' => 'postname', 'options' => array( 'permalink_structure' => '/%postname%/' ) ),
	array( 'name' => 'post_id', 'options' => array( 'permalink_structure' => '/archives/%post_id%' ) ),
	array(
		'name'    => 'date_and_bases',
		'options' => array(
			'permalink_structure' => '/%year%/%monthnum%/%day%/%postname%/',
			'category_base'       => 'topics',
			'tag_base'            => 'labels',
			'page_on_front'       => '12',
		),
	),
	array( 'name' => 'index_permalinks', 'options' => array( 'permalink_structure' => '/index.php/%postname%/' ) ),
	array(
		'name'    => 'subdirectory',
		'options' => array( 'permalink_structure' => '/%postname%/' ),
		'home'    => 'https://example.test/blog',
		'site'    => 'https://example.test/blog/wp',
	),
	array(
		'name'      => 'extras',
		'options'   => array( 'permalink_structure' => '/%postname%/' ),
		'endpoints' => array( array( 8191, 'json', 'json' ) ),
		'top'       => array( array( 'top/([^/]+)/?$', 'index.php?top=$matches[1]' ) ),
		'bottom'    => array( array( 'bottom/?$', 'index.php?bottom=1' ) ),
		'external'  => array( array( 'legacy/(.+?)$', 'legacy.php?path=$1' ) ),
	),
	array(
		'name'       => 'random_content',
		'options'    => array( 'permalink_structure' => '/%postname%/' ),
		'randomable' => array(
			array( 'name' => 'post', 'has_archive' => false, 'rewrite' => false ),
			array( 'name' => 'book', 'has_archive' => 'books', 'rewrite' => array( 'slug' => 'book' ) ),
			array( 'name' => 'movie', 'has_archive' => true, 'rewrite' => array( 'slug' => 'film' ) ),
			array( 'name' => 'quote', 'has_archive' => true, 'rewrite' => array( 'with_front' => true ) ),
			array( 'name' => 'note', 'has_archive' => 'notes', 'rewrite' => false ),
		),
		'taxonomies' => array(
			array( 'name' => 'category', 'rewrite' => array( 'slug' => 'category' ), 'object_type' => array( 'post' ) ),
			array( 'name' => 'post_tag', 'rewrite' => array( 'slug' => 'tag' ), 'object_type' => array( 'page' ) ),
			array( 'name' => 'genre', 'rewrite' => array( 'slug' => 'genre' ), 'object_type' => array( 'book', 'movie' ) ),
			array( 'name' => 'unregistered', 'rewrite' => array( 'slug' => 'nope' ), 'object_type' => array( 'book' ) ),
			array( 'name' => 'plain', 'rewrite' => false, 'object_type' => array( 'book' ) ),
		),
		'permastructs' => array( array( 'genre', 'genre/%genre%', '([^/]+)', 'genre=' ) ),
		'top'          => array( array( 'random/books/?$', 'index.php?post_type=book&orderby=rand' ) ),
	),
	array(
		'name'    => 'random_base_taken',
		'options' => array( 'permalink_structure' => '/%postname%/', 'wp_random_base' => 'surprise' ),
		'posts'   => array( array( 'surprise', 'post' ) ),
	),
	array(
		'name'    => 'random_base_page_taken',
		'options' => array( 'permalink_structure' => '/%year%/%postname%/' ),
		'posts'   => array( array( 'random', 'page' ), array( 'random', 'post' ) ),
	),
	array(
		'name'    => 'random_base_post_elsewhere',
		'options' => array( 'permalink_structure' => '/%year%/%postname%/' ),
		'posts'   => array( array( 'random', 'post' ) ),
	),
	array(
		'name'    => 'random_disabled',
		'options' => array( 'permalink_structure' => '/%postname%/' ),
		'random'  => false,
	),
	array(
		'name'    => 'verbose_rules',
		'options' => array( 'permalink_structure' => '/%postname%/' ),
		'verbose' => true,
		'home'    => 'https://example.test/blog',
		'site'    => 'https://example.test',
	),
);

$out = array();
foreach ( $cases as $case ) {
	$case['home'] = $case['home'] ?? 'https://example.test';
	$case['site'] = $case['site'] ?? 'https://example.test';
	$GLOBALS['case'] = $case;
	$rewrite = new WP_Rewrite();
	$rewrite->matches = 'matches';
	$rewrite->use_verbose_rules = ! empty( $case['verbose'] );
	taxonomies( $rewrite, $case['options'] );
	foreach ( $case['permastructs'] ?? array() as $permastruct ) {
		$rewrite->add_rewrite_tag( "%{$permastruct[0]}%", $permastruct[2], $permastruct[3] );
		$rewrite->add_permastruct( $permastruct[0], $permastruct[1], array( 'with_front' => true ) );
	}
	foreach ( $case['endpoints'] ?? array() as $endpoint ) {
		$rewrite->endpoints[] = $endpoint;
	}
	foreach ( $case['top'] ?? array() as $rule ) {
		$rewrite->add_rule( $rule[0], $rule[1], 'top' );
	}
	foreach ( $case['bottom'] ?? array() as $rule ) {
		$rewrite->add_rule( $rule[0], $rule[1], 'bottom' );
	}
	foreach ( $case['external'] ?? array() as $rule ) {
		$rewrite->add_rule( $rule[0], $rule[1] );
	}
	$rules = $rewrite->rewrite_rules();
	$pairs = array();
	foreach ( $rules as $regex => $query ) {
		$pairs[] = array( (string) $regex, $query );
	}
	$mod = $rewrite->mod_rewrite_rules();
	$out[] = array(
		'name'      => $case['name'],
		'options'   => (object) $case['options'],
		'home'      => $case['home'],
		'site'      => $case['site'],
		'verbose'   => ! empty( $case['verbose'] ),
		'endpoints' => $case['endpoints'] ?? array(),
		'top'       => $case['top'] ?? array(),
		'bottom'    => $case['bottom'] ?? array(),
		'external'  => $case['external'] ?? array(),
		'randomable'   => $case['randomable'] ?? array(),
		'taxonomies'   => $case['taxonomies'] ?? array(),
		'permastructs' => $case['permastructs'] ?? array(),
		'posts'        => $case['posts'] ?? array(),
		'random'       => $case['random'] ?? true,
		'rules'     => $pairs,
		'mod_rewrite' => $mod,
		'iis7'      => $rewrite->iis7_url_rewrite_rules( false ),
		'iis7_parent' => $rewrite->iis7_url_rewrite_rules( true ),
	);
}

echo json_encode( $out, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE ), "\n";
