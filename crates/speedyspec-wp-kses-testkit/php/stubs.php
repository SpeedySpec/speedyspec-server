<?php
// The WordPress functions kses.php calls that the oracle does not load, reduced to what kses needs.
// apply_filters() counts each run and returns the value unchanged: the oracle has no listeners.

$GLOBALS['__oracle_filters'] = array();
$GLOBALS['__oracle_current_filter'] = false;

function apply_filters( $hook_name, $value, ...$args ) {
	$GLOBALS['__oracle_filters'][ $hook_name ] = ( $GLOBALS['__oracle_filters'][ $hook_name ] ?? 0 ) + 1;
	return $value;
}
function do_action( $hook_name, ...$args ) {}
function did_action( $hook_name ) { return 0; }
function has_filter( $hook_name, $callback = false ) { return false; }
function add_filter( $hook_name, $callback, $priority = 10, $accepted_args = 1 ) { return true; }
function remove_filter( $hook_name, $callback, $priority = 10 ) { return true; }
function current_filter() { return $GLOBALS['__oracle_current_filter']; }
function current_user_can( $capability, ...$args ) { return false; }
function get_option( $option, $default_value = false ) { return 'blog_charset' === $option ? 'UTF-8' : $default_value; }
function is_admin() { return false; }
function __( $text, $domain = 'default' ) { return $text; }
function _deprecated_argument( $function_name, $version, $message = '' ) {}
function _deprecated_function( $function_name, $version, $replacement = '' ) {}
function _doing_it_wrong( $function_name, $message, $version ) {}
function wp_unslash( $value ) { return map_deep( $value, 'stripslashes_from_strings_only' ); }
function wp_slash( $value ) { return is_string( $value ) ? addslashes( $value ) : $value; }
function stripslashes_from_strings_only( $value ) { return is_string( $value ) ? stripslashes( $value ) : $value; }
function wp_upload_dir( $time = null, $create_dir = true, $refresh_cache = false ) {
	return array( 'url' => 'http://example.org:8080/wp-content/uploads', 'baseurl' => 'http://example.org:8080/wp-content/uploads' );
}
function wp_parse_url( $url, $component = -1 ) { return parse_url( $url, $component ); }
