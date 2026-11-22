// Evaluated without an opening tag. No strict_types: WordPress's own pluggable.php is not strict, so arguments coerce as they do there.

namespace SpeedySpec\WpPassword
{
    /**
     * Coerces a password or hash the way PHP's string functions coerce their arguments.
     *
     *  WordPress hands its arguments to `strlen()`, `trim()` and `hash_hmac()`, so a scalar becomes a string, `null`
     *  raises PHP's deprecation and an array a `TypeError`. The natives read only strings.
     *
     * @param string $value
     *   The argument as the caller passed it.
     * @return string
     *   The string WordPress's functions would have seen.
     */
    function password_string(string $value): string
    {
        return $value;
    }

    /**
     * Hashes an application password as `WP_Application_Passwords::hash_password()` does.
     *
     * @param string $password
     *   The plaintext application password.
     * @return string
     *   The `$generic$` fast hash.
     */
    function application_password_hash(
        #[\SensitiveParameter]
        string $password
    ): string {
        return Native\application_password_hash($password);
    }

    /**
     * Checks an application password as `WP_Application_Passwords::check_password()` does.
     *
     *  A `$generic$` hash is a fast hash; anything else goes to the bound `wp_check_password()`.
     *
     * @param string $password
     *   The plaintext application password.
     * @param string $hash
     *   The stored hash.
     * @return bool
     *   Whether the password matches.
     */
    function application_password_check(
        #[\SensitiveParameter]
        string $password,
        string $hash
    ): bool {
        return Native\application_password_check($password, $hash);
    }

    /**
     * Splits an application password into groups of four for display, as `WP_Application_Passwords::chunk_password()`.
     *
     * @param string $raw_password
     *   The application password.
     * @return string
     *   The password in space-separated groups of four.
     */
    function chunk_password(
        #[\SensitiveParameter]
        string $raw_password
    ): string {
        return Native\chunk_password($raw_password);
    }
}

namespace
{
    if (!function_exists('wp_hash_password'))
    {
        /**
         * Creates a hash of a plain text password, argon2id by default.
         *
         *  The implementation is the container's `SpeedySpec\WpPassword\PasswordHasher`, which PHP can rebind.
         *
         * @param string $password
         *   Plain text user password to hash.
         * @return string
         *   The hash string of the password.
         */
        function wp_hash_password(
            #[\SensitiveParameter]
            $password
        ) {
            return \SpeedySpec\WpPassword\Native\wp_hash_password(\SpeedySpec\WpPassword\password_string($password));
        }
    }

    if (!function_exists('wp_check_password'))
    {
        /**
         * Checks a plaintext password against a hashed password.
         *
         *  Unlike WordPress, raw MD5 and phpass hashes never match.
         *
         * @param string $password
         *   Plaintext password.
         * @param string $hash
         *   Hash of the password to check against.
         * @param string|int $user_id
         *   Optional ID of a user associated with the password.
         * @return bool
         *   Whether the password matches, as the `check_password` filter last said.
         */
        function wp_check_password(
            #[\SensitiveParameter]
            $password,
            $hash,
            $user_id = ''
        ) {
            return \SpeedySpec\WpPassword\Native\wp_check_password(
                \SpeedySpec\WpPassword\password_string($password),
                \SpeedySpec\WpPassword\password_string($hash),
                $user_id
            );
        }
    }

    if (!function_exists('wp_password_needs_rehash'))
    {
        /**
         * Checks whether a password hash needs to be rehashed.
         *
         * @param string $hash
         *   Hash of a password to check.
         * @param string|int $user_id
         *   Optional ID of a user associated with the password.
         * @return bool
         *   Whether the hash needs to be rehashed.
         */
        function wp_password_needs_rehash($hash, $user_id = '')
        {
            return \SpeedySpec\WpPassword\Native\wp_password_needs_rehash(\SpeedySpec\WpPassword\password_string($hash), $user_id);
        }
    }

    if (!function_exists('wp_generate_password'))
    {
        /**
         * Generates a random password drawn from the defined set of characters.
         *
         * @param int $length
         *   The length of password to generate.
         * @param bool $special_chars
         *   Whether to include standard special characters.
         * @param bool $extra_special_chars
         *   Whether to include other special characters.
         * @return string
         *   The random password, as the `random_password` filter last said.
         */
        function wp_generate_password($length = 12, $special_chars = true, $extra_special_chars = false)
        {
            return \SpeedySpec\WpPassword\Native\wp_generate_password($length, $special_chars, $extra_special_chars);
        }
    }

    if (!function_exists('wp_rand'))
    {
        /**
         * Generates a random non-negative number.
         *
         * @param int $min
         *   Lower limit for the generated number. Defaults to 0.
         * @param int $max
         *   Upper limit for the generated number. Defaults to 4294967295.
         * @return int
         *   A random non-negative number between min and max.
         */
        function wp_rand($min = null, $max = null)
        {
            if (null === $min)
            {
                $min = 0;
            }
            if (null === $max)
            {
                $max = 4294967295;
            }
            return \SpeedySpec\WpPassword\Native\wp_rand((int) $min, (int) $max);
        }
    }
}
