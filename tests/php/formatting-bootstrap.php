<?php

declare(strict_types=1);

/**
 * Loads what WordPress's formatting tests need around the native formatting functions: the stub test case, then
 * compat/formatting/setup.php in port mode, which loads the prepared release (formatting.php and kses.php without
 * their ported functions; speedyspec-wp-text-php prepares it) and answers what formatting reads (options,
 * translations, the locale, URLs) the way the recorded corpus does.
 *
 * Tests marked `@group needs-wordpress-core` need a booted WordPress (factories, posts, comments, the database) and
 * are excluded.
 */

require __DIR__ . '/WP_UnitTestCase.php';
require __DIR__ . '/includes/mock-action.php';
require __DIR__ . '/includes/mock-invokable.php';

if (!function_exists('wptexturize') || !function_exists('wp_kses') || !function_exists('add_filter')) {
    throw new RuntimeException('The native formatting, kses and Hook APIs are not loaded: run these tests in the embedded PHP.');
}

$speedyspec_root = getenv('SPEEDYSPEC_FORMATTING_WP_ROOT');
if (!$speedyspec_root) {
    throw new RuntimeException('Set SPEEDYSPEC_FORMATTING_WP_ROOT to the prepared WordPress release.');
}
define('SPEEDYSPEC_COMPAT_WP_ROOT', rtrim($speedyspec_root, '/') . '/');
define('SPEEDYSPEC_COMPAT_PORT', true);
// WordPress's test data the copied tests read (tests/phpunit/data in wordpress-develop).
define('DIR_TESTDATA', __DIR__ . '/wordpress/data');

require __DIR__ . '/../../compat/formatting/setup.php';
// The rest of the HTML API, for the tests' build_visual_html_tree() (setup.php loads the Tag Processor).
foreach (['class-wp-html-unsupported-exception', 'class-wp-html-active-formatting-elements', 'class-wp-html-open-elements',
    'class-wp-html-token', 'class-wp-html-stack-event', 'class-wp-html-processor-state', 'class-wp-html-processor'] as $speedyspec_file) {
    require ABSPATH . WPINC . "/html-api/{$speedyspec_file}.php";
}
require __DIR__ . '/includes/build-visual-html-tree.php';
require __DIR__ . '/includes/list-util.php';

/** A single site. */
function is_multisite() {
    return false;
}

/** WordPress's wp_list_pluck() (wp-includes/functions.php), over the copied WP_List_Util. */
function wp_list_pluck( $input_list, $field, $index_key = null ) {
    if ( ! is_array( $input_list ) ) {
        return array();
    }
    $util = new WP_List_Util( $input_list );
    return $util->pluck( $field, $index_key );
}

/**
 * update_option(): the bootstrap has no options table, so a test's change lasts through the pre_option filter
 * setup.php's get_option() applies (the hook snapshot removes it after each test).
 */
function update_option( $option, $value, $autoload = null ) {
    add_filter( "pre_option_{$option}", static fn () => $value );
    return true;
}

/** sanitizeTextField.php's provider passes one as an argument. */
class WP_Query {
}

// WordPress's benchmark_pcre_backtracking(), copied from tests/phpunit/includes/utils.php.
/**
 * Determine approximate backtrack count when running PCRE.
 *
 * @return int The backtrack count.
 */
function benchmark_pcre_backtracking( $pattern, $subject, $strategy ) {
	$saved_config = ini_get( 'pcre.backtrack_limit' );

	// Attempt to prevent PHP crashes. Adjust lower when needed.
	$limit = 1000000;

	// Start with small numbers, so if a crash is encountered at higher numbers we can still debug the problem.
	for ( $i = 4; $i <= $limit; $i *= 2 ) {

		ini_set( 'pcre.backtrack_limit', $i );

		switch ( $strategy ) {
			case 'split':
				preg_split( $pattern, $subject );
				break;
			case 'match':
				preg_match( $pattern, $subject );
				break;
			case 'match_all':
				$matches = array();
				preg_match_all( $pattern, $subject, $matches );
				break;
		}

		ini_set( 'pcre.backtrack_limit', $saved_config );

		switch ( preg_last_error() ) {
			case PREG_NO_ERROR:
				return $i;
			case PREG_BACKTRACK_LIMIT_ERROR:
				break;
			case PREG_RECURSION_LIMIT_ERROR:
				trigger_error( 'PCRE recursion limit encountered before backtrack limit.' );
				return;
			case PREG_BAD_UTF8_ERROR:
				trigger_error( 'UTF-8 error during PCRE benchmark.' );
				return;
			case PREG_INTERNAL_ERROR:
				trigger_error( 'Internal error during PCRE benchmark.' );
				return;
			default:
				trigger_error( 'Unexpected error during PCRE benchmark.' );
				return;
		}
	}

	return $i;
}
