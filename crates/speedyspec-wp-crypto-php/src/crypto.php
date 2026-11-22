// Evaluated without an opening tag. No strict_types: WordPress's own files are not strict, so arguments coerce as they
// do there.

namespace SpeedySpec\WpCrypto
{
    /**
     * Turns a native error marker into the WP_Error WordPress returns, with WordPress's data in WordPress's order.
     *
     *  `false` when WP_Error isn't loaded, as the other ported areas do: there is nothing to return the error in.
     *
     * @param array $marker
     *   The native function's error.
     * @param mixed $signatures
     *   The signatures as the caller passed them, which WordPress returns in its data.
     * @return \WP_Error|false
     *   The error.
     */
    function wp_error(array $marker, $signatures)
    {
        if (!class_exists('WP_Error', false))
        {
            return false;
        }
        if ('signature_verification_no_signature' === $marker['code'])
        {
            $data = array('filename' => $marker['filename']);
        }
        else
        {
            $data = array(
                'filename'    => $marker['filename'],
                'keys'        => $marker['keys'],
                'signatures'  => $signatures,
                'hash'        => $marker['hash'],
                'skipped_key' => $marker['skipped_key'],
                'skipped_sig' => $marker['skipped_sig'],
                'php'         => PHP_VERSION,
                'sodium'      => $marker['sodium'],
            );
        }
        return new \WP_Error($marker['code'], $marker['message'], $data);
    }

    /**
     * Gives each signature or key to the native check as a string, the way WordPress's `base64_decode()` call sees it.
     *
     *  Strings pass as they are. Anything else goes through PHP's own `base64_decode()` and back through
     *  `base64_encode()`, so its coercions, deprecations and `TypeError`s are PHP's, and the native decoder reads the
     *  same bytes.
     *
     * @param mixed $values
     *   The value WordPress casts with `(array)`.
     * @return string[]
     *   The strings, in order.
     */
    function strings($values): array
    {
        $out = array();
        foreach ((array) $values as $value)
        {
            $out[] = is_string($value) ? $value : base64_encode(base64_decode($value));
        }
        return $out;
    }
}

namespace
{
    if (!function_exists('wp_fast_hash'))
    {
        /**
         * Hashes a high-entropy message with keyed BLAKE2b, as WordPress 6.8's `wp_fast_hash()`.
         *
         * @param string $message
         *   The message to hash.
         * @return string
         *   The `$generic$` hash.
         */
        function wp_fast_hash(
            #[\SensitiveParameter]
            string $message
        ): string {
            return \SpeedySpec\WpCrypto\Native\wp_fast_hash($message);
        }
    }

    if (!function_exists('wp_verify_fast_hash'))
    {
        /**
         * Checks a message against a `wp_fast_hash()` hash.
         *
         *  Unlike WordPress, a phpass hash never verifies: SpeedySpec carries no phpass.
         *
         * @param string $message
         *   The plaintext message.
         * @param string $hash
         *   The hash to check against.
         * @return bool
         *   Whether the message matches.
         */
        function wp_verify_fast_hash(
            #[\SensitiveParameter]
            string $message,
            string $hash
        ): bool {
            return \SpeedySpec\WpCrypto\Native\wp_verify_fast_hash($message, $hash);
        }
    }

    if (!function_exists('wp_trusted_keys'))
    {
        /**
         * Retrieves the signing keys WordPress trusts, after the `wp_trusted_keys` filter.
         *
         * @return string[]
         *   The base64-encoded keys, as the filter returned them.
         */
        function wp_trusted_keys()
        {
            return \SpeedySpec\WpCrypto\Native\wp_trusted_keys();
        }
    }

    if (!function_exists('verify_file_signature'))
    {
        /**
         * Verifies a file against its Ed25519 signatures and the trusted keys.
         *
         * @param string       $filename
         *   The file to verify.
         * @param string|array $signatures
         *   The file's signatures, base64.
         * @param string|false $filename_for_errors
         *   A friendly file name for errors; the file's base name when falsy.
         * @return bool|WP_Error
         *   True when a signature verifies, otherwise the WP_Error describing why not.
         */
        function verify_file_signature($filename, $signatures, $filename_for_errors = false)
        {
            $result = \SpeedySpec\WpCrypto\Native\verify_file_signature(
                (string) $filename,
                $signatures ? \SpeedySpec\WpCrypto\strings($signatures) : null,
                $filename_for_errors ? (string) $filename_for_errors : null
            );
            return is_array($result) ? \SpeedySpec\WpCrypto\wp_error($result, $signatures) : $result;
        }
    }
}
