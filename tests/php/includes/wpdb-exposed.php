<?php

// Copied unchanged from wordpress-develop's tests/phpunit/includes/utils.php.

class WpdbExposedMethodsForTesting extends wpdb {
	public function __construct() {
		global $wpdb;
		$this->dbh         = $wpdb->dbh;
		$this->is_mysql    = $wpdb->is_mysql;
		$this->ready       = true;
		$this->field_types = $wpdb->field_types;
		$this->charset     = $wpdb->charset;

		$this->dbuser     = $wpdb->dbuser;
		$this->dbpassword = $wpdb->dbpassword;
		$this->dbname     = $wpdb->dbname;
		$this->dbhost     = $wpdb->dbhost;
	}

	public function __call( $name, $arguments ) {
		return call_user_func_array( array( $this, $name ), $arguments );
	}
}

// Copied unchanged from wordpress-develop's tests/phpunit/includes/utils.php.

/**
 * Returns a string of the required length containing random characters. Note that
 * the maximum possible string length is 32.
 *
 * @param int $length Optional. The required length. Default 32.
 * @return string The string.
 */
function rand_str( $length = 32 ) {
	return substr( md5( uniqid( rand() ) ), 0, $length );
}

// Copied unchanged from WordPress 7.1.5's wp-includes/functions.php: the caller wpdb::get_caller() reports.

/**
 * Returns a comma-separated string or array of functions that have been called to get
 * to the current point in code.
 *
 * @since 3.4.0
 *
 * @see https://core.trac.wordpress.org/ticket/19589
 *
 * @param string $ignore_class Optional. A class to ignore all function calls within - useful
 *                             when you want to just give info about the callee. Default null.
 * @param int    $skip_frames  Optional. A number of stack frames to skip - useful for unwinding
 *                             back to the source of the issue. Default 0.
 * @param bool   $pretty       Optional. Whether you want a comma separated string instead of
 *                             the raw array returned. Default true.
 * @return string|array Either a string containing a reversed comma separated trace or an array
 *                      of individual calls.
 */
function wp_debug_backtrace_summary( $ignore_class = null, $skip_frames = 0, $pretty = true ) {
	static $truncate_paths;

	$trace       = debug_backtrace( false );
	$caller      = array();
	$check_class = ! is_null( $ignore_class );
	++$skip_frames; // Skip this function.

	if ( ! isset( $truncate_paths ) ) {
		$truncate_paths = array(
			wp_normalize_path( WP_CONTENT_DIR ),
			wp_normalize_path( ABSPATH ),
		);
	}

	foreach ( $trace as $call ) {
		if ( $skip_frames > 0 ) {
			--$skip_frames;
		} elseif ( isset( $call['class'] ) ) {
			if ( $check_class && $ignore_class === $call['class'] ) {
				continue; // Filter out calls.
			}

			$caller[] = "{$call['class']}{$call['type']}{$call['function']}";
		} else {
			if ( in_array( $call['function'], array( 'do_action', 'apply_filters', 'do_action_ref_array', 'apply_filters_ref_array' ), true ) ) {
				$caller[] = "{$call['function']}('{$call['args'][0]}')";
			} elseif ( in_array( $call['function'], array( 'include', 'include_once', 'require', 'require_once' ), true ) ) {
				$filename = $call['args'][0] ?? '';
				$caller[] = $call['function'] . "('" . str_replace( $truncate_paths, '', wp_normalize_path( $filename ) ) . "')";
			} else {
				$caller[] = $call['function'];
			}
		}
	}
	if ( $pretty ) {
		return implode( ', ', array_reverse( $caller ) );
	} else {
		return $caller;
	}
}

/**
 * Normalizes a filesystem path.
 *
 * On windows systems, replaces backslashes with forward slashes
 * and forces upper-case drive letters.
 * Allows for two leading slashes for Windows network shares, but
 * ensures that all other duplicate slashes are reduced to a single.
 *
 * @since 3.9.0
 * @since 4.4.0 Ensures upper-case drive letters on Windows systems.
 * @since 4.5.0 Allows for Windows network shares.
 * @since 4.9.7 Allows for PHP file wrappers.
 * @since 7.0.0 Uses a static cache to store normalized paths.
 *
 * @param string $path Path to normalize.
 * @return string Normalized path.
 */
function wp_normalize_path( $path ): string {
	$path = (string) $path;

	static $cache = array();
	if ( isset( $cache[ $path ] ) ) {
		return $cache[ $path ];
	}

	$original_path = $path;
	$wrapper       = '';

	if ( wp_is_stream( $path ) ) {
		list( $wrapper, $path ) = explode( '://', $path, 2 );

		$wrapper .= '://';
	}

	// Standardize all paths to use '/'.
	$path = str_replace( '\\', '/', $path );

	// Replace multiple slashes down to a singular, allowing for network shares having two slashes.
	$path = (string) preg_replace( '|(?<=.)/+|', '/', $path );

	// Windows paths should uppercase the drive letter.
	if ( ':' === substr( $path, 1, 1 ) ) {
		$path = ucfirst( $path );
	}

	$cache[ $original_path ] = $wrapper . $path;
	return $cache[ $original_path ];
}

/**
 * Tests if a given path is a stream URL
 *
 * @since 3.5.0
 *
 * @param string $path The resource path or URL.
 * @return bool True if the path is a stream URL.
 */
function wp_is_stream( $path ) {
	$scheme_separator = strpos( $path, '://' );

	if ( false === $scheme_separator ) {
		// $path isn't a stream.
		return false;
	}

	$stream = substr( $path, 0, $scheme_separator );

	return in_array( $stream, stream_get_wrappers(), true );
}
