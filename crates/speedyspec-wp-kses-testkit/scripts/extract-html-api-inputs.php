<?php
// Collects every string literal containing '<' from WordPress's HTML API tests, as base64 JSON lines.
// Usage: php scripts/extract-html-api-inputs.php <wordpress-develop>/tests/phpunit/tests/html-api > corpus/wordpress/html-api-inputs.json
$out = array();
foreach ( glob( rtrim( $argv[1], '/' ) . '/*.php' ) as $file ) {
	foreach ( token_get_all( file_get_contents( $file ) ) as $t ) {
		if ( is_array( $t ) && T_CONSTANT_ENCAPSED_STRING === $t[0] ) {
			$v = @eval( 'return ' . $t[1] . ';' );
			if ( is_string( $v ) && str_contains( $v, '<' ) ) {
				$out[ $v ] = true;
			}
		}
	}
}
echo json_encode( array_map( 'base64_encode', array_keys( $out ) ) ), "\n";
