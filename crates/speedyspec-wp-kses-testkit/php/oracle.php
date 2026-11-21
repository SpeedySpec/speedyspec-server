<?php
// The PHP oracle: WordPress's own kses.php (from the pinned release given as argv[1]) answering one JSON request
// per line on stdin with one JSON response per line on stdout. Strings travel as {"b64": ...} so any bytes survive,
// arrays as {"array": [[key, value], ...]} so key order and key types survive.

error_reporting( E_ALL & ~E_DEPRECATED );
$wp = rtrim( $argv[1], '/' );
define( 'ABSPATH', $wp . '/' );
define( 'WPINC', 'wp-includes' );

require __DIR__ . '/stubs.php';

/** Defines WordPress's own copy of each named function from a release file. */
function oracle_load_functions( $file, array $names ) {
	$source = file_get_contents( $file );
	foreach ( $names as $name ) {
		if ( ! preg_match( '/^function ' . preg_quote( $name, '/' ) . '\(.*?^}\n/ms', $source, $m ) ) {
			fwrite( STDERR, "oracle: $name not found in $file\n" );
			exit( 2 );
		}
		eval( $m[0] );
	}
}

oracle_load_functions( "$wp/wp-includes/formatting.php", array( '_wp_specialchars', 'wp_check_invalid_utf8', 'esc_attr', 'map_deep' ) );
oracle_load_functions( "$wp/wp-includes/functions.php", array( 'wp_allowed_protocols' ) );
require "$wp/wp-includes/class-wp-token-map.php";
foreach ( array( 'html5-named-character-references', 'class-wp-html-attribute-token', 'class-wp-html-span', 'class-wp-html-doctype-info', 'class-wp-html-text-replacement', 'class-wp-html-decoder', 'class-wp-html-tag-processor' ) as $html_api_file ) {
	require "$wp/wp-includes/html-api/$html_api_file.php";
}
require "$wp/wp-includes/kses.php";

function oracle_encode( $v ) {
	if ( is_string( $v ) ) {
		return array( 'b64' => base64_encode( $v ) );
	}
	if ( is_array( $v ) ) {
		$pairs = array();
		foreach ( $v as $k => $item ) {
			$pairs[] = array( oracle_encode( $k ), oracle_encode( $item ) );
		}
		return array( 'array' => $pairs );
	}
	if ( is_object( $v ) ) {
		return array( 'object' => get_class( $v ) );
	}
	return $v;
}

function oracle_decode( $v ) {
	if ( is_array( $v ) && array_key_exists( 'b64', $v ) ) {
		return base64_decode( $v['b64'] );
	}
	if ( is_array( $v ) && array_key_exists( 'array', $v ) ) {
		$out = array();
		foreach ( $v['array'] as $pair ) {
			$out[ oracle_decode( $pair[0] ) ] = oracle_decode( $pair[1] );
		}
		return $out;
	}
	if ( is_array( $v ) ) {
		return array_map( 'oracle_decode', $v );
	}
	return $v;
}

while ( false !== ( $line = fgets( STDIN ) ) ) {
	$request = json_decode( $line, true );
	$GLOBALS['__oracle_filters'] = array();
	$GLOBALS['__oracle_current_filter'] = $request['current_filter'] ?? false;
	$fn   = $request['fn'];
	$args = array_map( 'oracle_decode', $request['args'] ?? array() );
	try {
		if ( 'globals' === $fn ) {
			$returned = array(
				'allowedposttags'       => oracle_encode( $GLOBALS['allowedposttags'] ),
				'allowedtags'           => oracle_encode( $GLOBALS['allowedtags'] ),
				'allowedentitynames'    => oracle_encode( $GLOBALS['allowedentitynames'] ),
				'allowedxmlentitynames' => oracle_encode( $GLOBALS['allowedxmlentitynames'] ),
			);
		} elseif ( 'html_decode' === $fn ) {
			$returned = oracle_encode( WP_HTML_Decoder::decode( $args[0], $args[1] ) );
		} elseif ( 'html_attributes' === $fn ) {
			// The attributes of "<wp {$attr}>" as wp_kses_hair() reads them: comparable name => true or decoded value.
			$processor = new WP_HTML_Tag_Processor( "<wp {$args[0]}>" );
			$processor->next_token();
			$names = $processor->get_attribute_names_with_prefix( '' );
			if ( null === $names ) {
				$returned = null;
			} else {
				$pairs = array();
				foreach ( $names as $name ) {
					$pairs[] = array( $name, $processor->get_attribute( $name ) );
				}
				$returned = oracle_encode( $pairs );
			}
		} elseif ( 'html_named_references' === $fn ) {
			global $html5_named_character_references;
			$returned = oracle_encode( $html5_named_character_references->to_array() );
		} elseif ( 'preg' === $fn ) {
			$ok       = preg_match( $args[0], $args[1], $m, PREG_OFFSET_CAPTURE );
			$returned = oracle_encode( false === $ok ? null : ( $ok ? $m : array() ) );
		} else {
			$returned = oracle_encode( $fn( ...$args ) );
		}
		$filters = array();
		foreach ( $GLOBALS['__oracle_filters'] as $name => $count ) {
			$filters[] = array( $name, $count );
		}
		$response = array( 'returned' => $returned, 'filters' => $filters );
	} catch ( Throwable $e ) {
		$response = array( 'error' => get_class( $e ) . ': ' . $e->getMessage() );
	}
	echo json_encode( $response, JSON_THROW_ON_ERROR ), "\n";
	flush();
}
