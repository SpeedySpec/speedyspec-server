<?php
/**
 * Loads WordPress's formatting functions for the `formatting` compatibility area.
 *
 * The formatting functions read a few outside facts (options, translations, the locale) and report deprecations.
 * Here each is answered the way the Rust port's in-memory adapters answer it (`speedyspec-wp-text-memory`), so a
 * recorded result is what the port must produce with those adapters. Each step's diagnostics are reported as its
 * `state`.
 */

declare(strict_types=1);

if (!SPEEDYSPEC_COMPAT_PORT) {
    define('ABSPATH', SPEEDYSPEC_COMPAT_WP_ROOT);
    define('WPINC', 'wp-includes');

    $GLOBALS['speedyspec_formatting_diagnostics'] = [];

    /**
     * Configures the driver, which loads after this file and calls this before the steps run.
     */
    function speedyspec_compat_area_setup(): void
    {
        SpeedySpecCompat::$exact_bytes = true;
        SpeedySpecCompat::$report_globals = false;
        SpeedySpecCompat::$state = static fn () => $GLOBALS['speedyspec_formatting_diagnostics'];
    }

    function get_option($option, $default_value = false)
    {
        return match ($option) {
            'blog_charset' => 'UTF-8',
            'use_smilies' => '1',
            'use_balanceTags' => '0',
            default => $default_value,
        };
    }

    function __($text, $domain = 'default')
    {
        return $text;
    }

    function _x($text, $context, $domain = 'default')
    {
        return $text;
    }

    function translate($text, $domain = 'default')
    {
        return $text;
    }

    function translate_with_gettext_context($text, $context, $domain = 'default')
    {
        return $text;
    }

    function get_locale()
    {
        return 'en_US';
    }

    function wp_get_word_count_type()
    {
        return 'words';
    }

    function includes_url($path = '', $scheme = null)
    {
        return 'https://example.org/wp-includes/' . $path;
    }

    function _deprecated_function($function_name, $version, $replacement = '')
    {
        $GLOBALS['speedyspec_formatting_diagnostics'][] = ['deprecated_function', $function_name, $version, $replacement];
    }

    function _deprecated_argument($function_name, $version, $message = '')
    {
        $GLOBALS['speedyspec_formatting_diagnostics'][] = ['deprecated_argument', $function_name, $version, $message];
    }

    function wp_trigger_error($function_name, $message, $error_level = E_USER_NOTICE)
    {
        $GLOBALS['speedyspec_formatting_diagnostics'][] = ['trigger_error', $function_name, $message, $error_level];
    }

    // Copied verbatim from WordPress 7.1.5's wp-includes/functions.php (is_utf8_charset, _canonical_charset,
    // mbstring_binary_safe_encoding, reset_mbstring_encoding) and deprecated.php (addslashes_gpc). Those files cannot
    // be required here: they need the real options API, which the stubs above stand in for.
    function is_utf8_charset( $blog_charset = null ) {
    	return _is_utf8_charset( $blog_charset ?? get_option( 'blog_charset' ) );
    }

    function _canonical_charset( $charset ) {
    	if ( is_utf8_charset( $charset ) ) {
    		return 'UTF-8';
    	}

    	/*
    	 * Normalize the ISO-8859-1 family of languages.
    	 *
    	 * This is not required for htmlspecialchars(), as it properly recognizes all of
    	 * the input character sets that here are transformed into "ISO-8859-1".
    	 *
    	 * @todo Should this entire check be removed since it's not required for the stated purpose?
    	 * @todo Should WordPress transform other potential charset equivalents, such as "latin1"?
    	 */
    	if (
    		( 0 === strcasecmp( 'iso-8859-1', $charset ) ) ||
    		( 0 === strcasecmp( 'iso8859-1', $charset ) )
    	) {
    		return 'ISO-8859-1';
    	}

    	return $charset;
    }

    function mbstring_binary_safe_encoding( $reset = false ) {
    	static $encodings  = array();
    	static $overloaded = null;

    	if ( is_null( $overloaded ) ) {
    		if ( function_exists( 'mb_internal_encoding' )
    			&& ( (int) ini_get( 'mbstring.func_overload' ) & 2 ) // phpcs:ignore PHPCompatibility.IniDirectives.RemovedIniDirectives.mbstring_func_overloadDeprecated
    		) {
    			$overloaded = true;
    		} else {
    			$overloaded = false;
    		}
    	}

    	if ( false === $overloaded ) {
    		return;
    	}

    	if ( ! $reset ) {
    		$encoding = mb_internal_encoding();
    		array_push( $encodings, $encoding );
    		mb_internal_encoding( 'ISO-8859-1' );
    	}

    	if ( $reset && $encodings ) {
    		$encoding = array_pop( $encodings );
    		mb_internal_encoding( $encoding );
    	}
    }

    function reset_mbstring_encoding() {
    	mbstring_binary_safe_encoding( true );
    }

    function addslashes_gpc( $gpc ) {
    	_deprecated_function( __FUNCTION__, '7.0.0', 'wp_slash()' );
    	return wp_slash( $gpc );
    }

    require ABSPATH . WPINC . '/compat-utf8.php';
    require ABSPATH . WPINC . '/compat.php';
    require ABSPATH . WPINC . '/utf8.php';
    require ABSPATH . WPINC . '/plugin.php';
    require ABSPATH . WPINC . '/kses.php';
    require ABSPATH . WPINC . '/formatting.php';
    require ABSPATH . WPINC . '/shortcodes.php';

    // Formatting's default callbacks on the hooks its own functions fire, copied from WordPress 7.1.5's
    // wp-includes/default-filters.php. The rest of formatting's share of that file is on hooks other seams fire;
    // `register_text_filters` and its tests cover those.
    add_filter( 'sanitize_title', 'sanitize_title_with_dashes', 10, 3 );
}
