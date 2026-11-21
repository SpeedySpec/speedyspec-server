<?php
// Collects the #data section of every html5lib tree-construction test, as base64 JSON.
// Usage: php scripts/extract-html5lib.php <html5lib-tests>/tree-construction > corpus/html5lib/tree-construction-data.json
$out = array();
foreach ( glob( rtrim( $argv[1], '/' ) . '/*.dat' ) as $file ) {
	$text = file_get_contents( $file );
	if ( preg_match_all( '/^#data\n(.*?)\n#errors/ms', $text, $m ) ) {
		foreach ( $m[1] as $data ) {
			$out[ $data ] = true;
		}
	}
}
echo json_encode( array_map( 'base64_encode', array_keys( $out ) ) ), "\n";
