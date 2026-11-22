// WordPress's formatting functions are native (speedyspec-wp-text-php); only the one with a by-reference parameter
// needs a PHP declaration, which hands its result back by reference.

function wp_parse_str( $input_string, &$result ) {
	$result = \SpeedySpec\WpText\Native\wp_parse_str( $input_string );
}
