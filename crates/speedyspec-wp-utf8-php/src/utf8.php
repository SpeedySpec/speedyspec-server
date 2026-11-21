// Evaluated without an opening tag. No strict_types: WordPress's own utf8.php and compat.php are not strict, so
// arguments coerce as they do there.

namespace SpeedySpec\WpUtf8
{
    /**
     * Coerces `_mb_substr()`'s arguments as WordPress's `_wp_utf8_codepoint_count()` and `_wp_utf8_codepoint_span()`
     *  parameters would, then takes the substring natively.
     */
    function mb_substr_utf8(string $str, int $start, ?int $length): string
    {
        return Native\mb_substr($str, $start, $length);
    }

    /**
     * Coerces `_mb_strlen()`'s argument as `_wp_utf8_codepoint_count()`'s `string` parameter would.
     */
    function mb_strlen_utf8(string $str): int
    {
        return Native\mb_strlen($str);
    }
}

namespace
{
    function wp_is_valid_utf8( string $bytes ): bool
    {
        return \SpeedySpec\WpUtf8\Native\is_valid_utf8( $bytes );
    }

    function wp_scrub_utf8( $text )
    {
        if ( ! is_string( $text ) ) {
            // What WordPress does for anything else: mb_scrub()'s own coercion, warnings and errors.
            $prev_replacement_character = mb_substitute_character();
            mb_substitute_character( 0xFFFD );
            $scrubbed = mb_scrub( $text, 'UTF-8' );
            mb_substitute_character( $prev_replacement_character );
            return $scrubbed;
        }
        return \SpeedySpec\WpUtf8\Native\scrub_utf8( $text );
    }

    function wp_has_noncharacters( string $text ): bool
    {
        return \SpeedySpec\WpUtf8\Native\has_noncharacters( $text );
    }

    function _wp_scan_utf8( string $bytes, int &$at, int &$invalid_length, ?int $max_bytes = null, ?int $max_code_points = null, ?bool &$has_noncharacters = null ): int
    {
        list( $count, $at, $invalid_length, $found, $tested ) = \SpeedySpec\WpUtf8\Native\scan_utf8( $bytes, $at, $max_bytes, $max_code_points );
        // WordPress's `$has_noncharacters |= …` leaves an integer once it has run.
        $has_noncharacters = $tested ? (int) $found : false;
        return $count;
    }

    function _wp_is_valid_utf8_fallback( string $bytes ): bool
    {
        return \SpeedySpec\WpUtf8\Native\is_valid_utf8_fallback( $bytes );
    }

    function _wp_scrub_utf8_fallback( string $bytes ): string
    {
        return \SpeedySpec\WpUtf8\Native\scrub_utf8( $bytes );
    }

    function _wp_utf8_codepoint_count( string $text, ?int $byte_offset = 0, ?int $max_byte_length = PHP_INT_MAX ): int
    {
        if ( $byte_offset < 0 ) {
            return 0;
        }
        $end             = strlen( $text );
        $max_byte_length = min( $end - $byte_offset, $max_byte_length );
        if ( ! ( $byte_offset < $end && 0 < $max_byte_length ) ) {
            return 0;
        }
        if ( null === $byte_offset ) {
            // WordPress passes a null offset on to _wp_scan_utf8(), whose `int &$at` refuses it.
            $at             = null;
            $invalid_length = 0;
            _wp_scan_utf8( $text, $at, $invalid_length );
        }
        return \SpeedySpec\WpUtf8\Native\codepoint_count( $text, $byte_offset, $max_byte_length );
    }

    function _wp_utf8_codepoint_span( string $text, int $byte_offset, int $max_code_points, ?int &$found_code_points = 0 ): int
    {
        list( $bytes, $found_code_points ) = \SpeedySpec\WpUtf8\Native\codepoint_span( $text, $byte_offset, $max_code_points );
        return $bytes;
    }

    function _wp_has_noncharacters_fallback( string $text ): bool
    {
        _deprecated_function( __FUNCTION__, '7.1.0' );
        return wp_has_noncharacters( $text );
    }

    function _wp_utf8_encode_fallback( $iso_8859_1_text )
    {
        return \SpeedySpec\WpUtf8\Native\utf8_encode( (string) $iso_8859_1_text );
    }

    function _wp_utf8_decode_fallback( $utf8_text )
    {
        return \SpeedySpec\WpUtf8\Native\utf8_decode( (string) $utf8_text );
    }

    function _is_utf8_charset( $charset_slug )
    {
        if ( ! is_string( $charset_slug ) ) {
            return false;
        }
        return \SpeedySpec\WpUtf8\Native\is_utf8_charset( $charset_slug );
    }

    function _mb_substr( $str, $start, $length = null, $encoding = null )
    {
        if ( null === $str ) {
            return '';
        }
        if ( ! _is_utf8_charset( $encoding ?? get_option( 'blog_charset' ) ) ) {
            return is_null( $length ) ? substr( $str, $start ) : substr( $str, $start, $length );
        }
        return \SpeedySpec\WpUtf8\mb_substr_utf8( $str, $start, $length );
    }

    function _mb_strlen( $str, $encoding = null )
    {
        return _is_utf8_charset( $encoding ?? get_option( 'blog_charset' ) )
            ? \SpeedySpec\WpUtf8\mb_strlen_utf8( $str )
            : strlen( $str );
    }
}
