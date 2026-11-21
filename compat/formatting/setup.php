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

    // __ and _x fire gettext and gettext_with_context as WordPress's do, so a test's translation filter works;
    // with none registered they return the text.
    function __($text, $domain = 'default')
    {
        return apply_filters('gettext', $text, $text, $domain);
    }

    function _x($text, $context, $domain = 'default')
    {
        return apply_filters('gettext_with_context', $text, $text, $context, $domain);
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
        return $GLOBALS['speedyspec_word_count_type'] ?? 'words';
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

    // The shortcodes a booted WordPress 7.1.5 has, in its registration order: media.php registers wp_caption,
    // caption, gallery, playlist, audio and video (wp-settings.php line 265), and WP_Embed's constructor registers
    // embed (wp-settings.php line 468). Their handlers are placeholders; formatting only reads the names.
    function speedyspec_shortcode_placeholder()
    {
        return '';
    }

    foreach (['wp_caption', 'caption', 'gallery', 'playlist', 'audio', 'video', 'embed'] as $tag) {
        add_shortcode($tag, 'speedyspec_shortcode_placeholder');
    }

    /**
     * wptexturize() under one of wpTexturize.php's translation filters, as test_translate (table 1, the test's
     * filter_translate) and test_primes_quotes_translation (table 2, filter_translate2) run it.
     *
     * @param string $text
     *     The text to texturize.
     * @param int $table
     *     1 or 2.
     * @return string
     *     The texturized text.
     */
    function speedyspec_wptexturize_translated($text, $table)
    {
        $filter = 1 === $table ? 'speedyspec_filter_translate' : 'speedyspec_filter_translate2';
        add_filter('gettext_with_context', $filter, 10, 4);
        $result = wptexturize($text, true);
        remove_filter('gettext_with_context', $filter, 10);
        wptexturize('reset', true);
        return $result;
    }

    // Copied verbatim from wpTexturize.php's filter_translate().
    function speedyspec_filter_translate( $translations, $text, $context, $domain ) {
		switch ( $text ) {
			case '&#8211;':
				return '!endash!';
			case '&#8212;':
				return '!emdash!';
			case '&#8216;':
				return '!openq1!';
			case '&#8217;':
				if ( 'apostrophe' === $context ) {
					return '!apos!';
				} else {
					return '!closeq1!';
				}
			case '&#8220;':
				return '!openq2!';
			case '&#8221;':
				return '!closeq2!';
			case '&#8242;':
				return '!prime1!';
			case '&#8243;':
				return '!prime2!';
			case '&#8217;tain&#8217;t,&#8217;twere,&#8217;twas,&#8217;tis,&#8217;twill,&#8217;til,&#8217;bout,&#8217;nuff,&#8217;round,&#8217;cause,&#8217;em':
				return '!apos!tain!apos!t,!apos!twere,!apos!twas,!apos!tis,!apos!twill,!apos!til,!apos!bout,!apos!nuff,!apos!round,!apos!cause,!apos!em';
			default:
				return $translations;
		}
	}

    // Copied verbatim from wpTexturize.php's filter_translate2().
    function speedyspec_filter_translate2( $translations, $text, $context, $domain ) {
		switch ( $text ) {
			case '&#8211;':
				return '!endash!';
			case '&#8212;':
				return '!emdash!';
			case '&#8216;':
				return '!q1!';
			case '&#8217;':
				if ( 'apostrophe' === $context ) {
					return '!apos!';
				} else {
					return '!q1!';
				}
			case '&#8220;':
				return '!q2!';
			case '&#8221;':
				return '!q2!';
			case '&#8242;':
				return '!prime1!';
			case '&#8243;':
				return '!prime2!';
			default:
				return $translations;
		}
	}

    /**
     * wptexturize() with wpTexturize.php's filter_shortcodes() on no_texturize_shortcodes, as
     * test_unregistered_shortcodes runs it.
     *
     * @param string $text
     *     The text to texturize.
     * @return string
     *     The texturized text.
     */
    function speedyspec_wptexturize_no_texturize_shortcodes($text)
    {
        add_filter('no_texturize_shortcodes', 'speedyspec_filter_shortcodes', 10, 1);
        $result = wptexturize($text);
        remove_filter('no_texturize_shortcodes', 'speedyspec_filter_shortcodes', 10);
        return $result;
    }

    // Copied from wpTexturize.php's filter_shortcodes().
    function speedyspec_filter_shortcodes( $disabled ) {
		$disabled[] = 'audio';
		return $disabled;
	}

    /**
     * wp_trim_words() with a word count type that counts characters, as wpTrimWords.php's ja_JP tests run it
     * (ja_JP's type is characters_including_spaces).
     *
     * @param mixed $text
     *     The text.
     * @param mixed $num_words
     *     The number of characters.
     * @param mixed $more
     *     What to append.
     * @return string
     *     The trimmed text.
     */
    function speedyspec_wp_trim_words_characters($text, $num_words = 55, $more = null)
    {
        $GLOBALS['speedyspec_word_count_type'] = 'characters_including_spaces';
        try {
            return wp_trim_words($text, $num_words, $more);
        } finally {
            unset($GLOBALS['speedyspec_word_count_type']);
        }
    }

    /**
     * capital_P_dangit() while $filter is the current filter, as capitalPDangit.php sets $wp_current_filter.
     *
     * @param mixed $text
     *     The text.
     * @param string $filter
     *     The filter to pretend is running.
     * @return string
     *     The corrected text.
     */
    function speedyspec_capital_P_dangit_in_filter($text, $filter)
    {
        global $wp_current_filter;
        $saved = $wp_current_filter;
        $wp_current_filter = [$filter];
        try {
            return capital_P_dangit($text);
        } finally {
            $wp_current_filter = $saved;
        }
    }
}
