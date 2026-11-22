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

// Both sides run this file. With `SPEEDYSPEC_COMPAT_PORT` true the release is the port's prepared copy (formatting.php
// and kses.php without their ported functions), the hooks are native so plugin.php is not loaded, and copied helpers
// a native area already declares are skipped.
if (true) {
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

    function home_url($path = '', $scheme = null)
    {
        return 'http://example.org' . ($path ? '/' . ltrim($path, '/') : '');
    }

    function site_url($path = '', $scheme = null)
    {
        return 'http://example.org' . ($path ? '/' . ltrim($path, '/') : '');
    }

    // set_url_scheme() for an explicit scheme, as makeClickable.php's provider calls it on home_url().
    function set_url_scheme($url, $scheme = null)
    {
        return preg_replace('#^\w+://#', $scheme . '://', $url);
    }

    function current_user_can($capability, ...$args)
    {
        return false;
    }

    require ABSPATH . WPINC . '/compat-utf8.php';
    require ABSPATH . WPINC . '/compat.php';
    require ABSPATH . WPINC . '/utf8.php';
    if (!SPEEDYSPEC_COMPAT_PORT) {
        require ABSPATH . WPINC . '/plugin.php';
    }
    // wp_kses_hair() reads attributes with the HTML Tag Processor; these are the files wp-settings.php loads for it.
    require ABSPATH . WPINC . '/class-wp-token-map.php';
    foreach (['html5-named-character-references', 'class-wp-html-attribute-token', 'class-wp-html-span',
        'class-wp-html-doctype-info', 'class-wp-html-text-replacement', 'class-wp-html-decoder',
        'class-wp-html-tag-processor'] as $speedyspec_file) {
        require ABSPATH . WPINC . "/html-api/{$speedyspec_file}.php";
    }
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
        // Run it as a callback of $filter, so current_filter() answers $filter with WordPress's hooks or native ones.
        add_filter($filter, 'capital_P_dangit', 11);
        try {
            return apply_filters($filter, $text);
        } finally {
            remove_filter($filter, 'capital_P_dangit', 11);
        }
    }

    // What plan 4's corpus needs from outside formatting.php: the URL, link, MIME and smiley helpers it calls, copied
    // verbatim below, then the smiley globals built once as a booted site has them.
    // Copied verbatim from WordPress 7.1.5's wp-includes/http.php.
    /**
     * A wrapper for PHP's parse_url() function that handles consistency in the return values
     * across PHP versions.
     *
     * Across various PHP versions, schemeless URLs containing a ":" in the query
     * are being handled inconsistently. This function works around those differences.
     *
     * @since 4.4.0
     * @since 4.7.0 The `$component` parameter was added for parity with PHP's `parse_url()`.
     *
     * @link https://www.php.net/manual/en/function.parse-url.php
     *
     * @param string $url       The URL to parse.
     * @param int    $component The specific component to retrieve. Use one of the PHP
     *                          predefined constants to specify which one.
     *                          Defaults to -1 (= return all parts as an array).
     * @return mixed False on parse failure; Array of URL components on success;
     *               When a specific component has been requested: null if the component
     *               doesn't exist in the given URL; a string or - in the case of
     *               PHP_URL_PORT - integer when it does. See parse_url()'s return values.
     *
     * @phpstan-param int<-1, 7> $component
     * @phpstan-return (
     *     $component is -1
     *         ? false|array{
     *               scheme?: string,
     *               host?: string,
     *               port?: int<0, 65535>,
     *               user?: string,
     *               pass?: string,
     *               path?: string,
     *               query?: string,
     *               fragment?: string,
     *           }
     *         : (
     *             $component is 2
     *                 ? int<0, 65535>|null
     *                 : string|null
     *         )
     * )
     */
    function wp_parse_url( $url, $component = -1 ) {
    	$to_unset = array();
    	$url      = (string) $url;

    	if ( str_starts_with( $url, '//' ) ) {
    		$to_unset[] = 'scheme';
    		$url        = 'placeholder:' . $url;
    	} elseif ( str_starts_with( $url, '/' ) ) {
    		$to_unset[] = 'scheme';
    		$to_unset[] = 'host';
    		$url        = 'placeholder://placeholder' . $url;
    	}

    	$parts = parse_url( $url );

    	if ( false === $parts ) {
    		// Parsing failure.
    		return $parts;
    	}

    	// Remove the placeholder values.
    	foreach ( $to_unset as $key ) {
    		unset( $parts[ $key ] );
    	}

    	return _get_component_from_parsed_url_array( $parts, $component );
    }

    // Copied verbatim from WordPress 7.1.5's wp-includes/http.php.
    /**
     * Retrieves a specific component from a parsed URL array.
     *
     * @internal
     *
     * @since 4.7.0
     * @access private
     *
     * @link https://www.php.net/manual/en/function.parse-url.php
     *
     * @param array|false $url_parts The parsed URL. Can be false if the URL failed to parse.
     * @param int         $component The specific component to retrieve. Use one of the PHP
     *                               predefined constants to specify which one.
     *                               Defaults to -1 (= return all parts as an array).
     * @return mixed False on parse failure; Array of URL components on success;
     *               When a specific component has been requested: null if the component
     *               doesn't exist in the given URL; a string or - in the case of
     *               PHP_URL_PORT - integer when it does. See parse_url()'s return values.
     *
     * @phpstan-param false|array{
     *     scheme?: string,
     *     host?: string,
     *     port?: int<0, 65535>,
     *     user?: string,
     *     pass?: string,
     *     path?: string,
     *     query?: string,
     *     fragment?: string,
     * } $url_parts
     * @phpstan-param int<-1, 7> $component
     * @phpstan-return (
     *     $component is -1
     *         ? false|array{
     *               scheme?: string,
     *               host?: string,
     *               port?: int<0, 65535>,
     *               user?: string,
     *               pass?: string,
     *               path?: string,
     *               query?: string,
     *               fragment?: string,
     *           }
     *         : (
     *             $component is 2
     *                 ? int<0, 65535>|null
     *                 : string|null
     *         )
     * )
     */
    function _get_component_from_parsed_url_array( $url_parts, $component = -1 ) {
    	if ( -1 === $component ) {
    		return $url_parts;
    	}

    	$key = _wp_translate_php_url_constant_to_key( $component );
    	if ( false !== $key && is_array( $url_parts ) && isset( $url_parts[ $key ] ) ) {
    		return $url_parts[ $key ];
    	} else {
    		return null;
    	}
    }

    // Copied verbatim from WordPress 7.1.5's wp-includes/http.php.
    /**
     * Translates a PHP_URL_* constant to the named array keys PHP uses.
     *
     * @internal
     *
     * @since 4.7.0
     * @access private
     *
     * @link https://www.php.net/manual/en/url.constants.php
     *
     * @param int $constant PHP_URL_* constant.
     * @return string|false The named key or false.
     *
     * @phpstan-param int<-1, 7> $constant
     * @phpstan-return 'scheme'|'host'|'port'|'user'|'pass'|'path'|'query'|'fragment'|false
     */
    function _wp_translate_php_url_constant_to_key( $constant ) {
    	$translation = array(
    		PHP_URL_SCHEME   => 'scheme',
    		PHP_URL_HOST     => 'host',
    		PHP_URL_PORT     => 'port',
    		PHP_URL_USER     => 'user',
    		PHP_URL_PASS     => 'pass',
    		PHP_URL_PATH     => 'path',
    		PHP_URL_QUERY    => 'query',
    		PHP_URL_FRAGMENT => 'fragment',
    	);

    	return $translation[ $constant ] ?? false;
    }

    // Copied verbatim from WordPress 7.1.5's wp-includes/link-template.php.
    /**
     * Returns an array of URL hosts which are considered to be internal hosts.
     *
     * By default the list of internal hosts is comprised of the host name of
     * the site's home_url() (as parsed by wp_parse_url()).
     *
     * This list is used when determining if a specified URL is a link to a page on
     * the site itself or a link offsite (to an external host). This is used, for
     * example, when determining if the "nofollow" attribute should be applied to a
     * link.
     *
     * @see wp_is_internal_link
     *
     * @since 6.2.0
     *
     * @return string[] An array of URL hosts.
     */
    function wp_internal_hosts() {
    	static $internal_hosts;

    	if ( empty( $internal_hosts ) ) {
    		/**
    		 * Filters the array of URL hosts which are considered internal.
    		 *
    		 * @since 6.2.0
    		 *
    		 * @param string[] $internal_hosts An array of internal URL hostnames.
    		 */
    		$internal_hosts = apply_filters(
    			'wp_internal_hosts',
    			array(
    				wp_parse_url( home_url(), PHP_URL_HOST ),
    			)
    		);
    		$internal_hosts = array_unique(
    			array_map( 'strtolower', (array) $internal_hosts )
    		);
    	}

    	return $internal_hosts;
    }

    // Copied verbatim from WordPress 7.1.5's wp-includes/link-template.php.
    /**
     * Determines whether or not the specified URL is of a host included in the internal hosts list.
     *
     * @see wp_internal_hosts()
     *
     * @since 6.2.0
     *
     * @param string $link The URL to test.
     * @return bool Returns true for internal URLs and false for all other URLs.
     */
    function wp_is_internal_link( $link ) {
    	$link = strtolower( $link );
    	if ( in_array( wp_parse_url( $link, PHP_URL_SCHEME ), wp_allowed_protocols(), true ) ) {
    		return in_array( wp_parse_url( $link, PHP_URL_HOST ), wp_internal_hosts(), true );
    	}
    	return false;
    }

    // Copied verbatim from WordPress 7.1.5's wp-includes/functions.php.
    /**
     * Retrieves the list of mime types and file extensions.
     *
     * @since 3.5.0
     * @since 4.2.0 Support was added for GIMP (.xcf) files.
     * @since 4.9.2 Support was added for Flac (.flac) files.
     * @since 4.9.6 Support was added for AAC (.aac) files.
     * @since 6.8.0 Support was added for `audio/x-wav`.
     *
     * @return string[] Array of mime types keyed by the file extension regex corresponding to those types.
     */
    function wp_get_mime_types() {
    	/**
    	 * Filters the list of mime types and file extensions.
    	 *
    	 * This filter should be used to add, not remove, mime types. To remove
    	 * mime types, use the {@see 'upload_mimes'} filter.
    	 *
    	 * @since 3.5.0
    	 *
    	 * @param string[] $wp_get_mime_types Mime types keyed by the file extension regex
    	 *                                    corresponding to those types.
    	 */
    	return apply_filters(
    		'mime_types',
    		array(
    			// Image formats.
    			'jpg|jpeg|jpe'                 => 'image/jpeg',
    			'gif'                          => 'image/gif',
    			'png'                          => 'image/png',
    			'bmp'                          => 'image/bmp',
    			'tiff|tif'                     => 'image/tiff',
    			'webp'                         => 'image/webp',
    			'avif'                         => 'image/avif',
    			'ico'                          => 'image/x-icon',

    			// TODO: Needs improvement. All images with the following mime types seem to have .heic file extension.
    			'heic'                         => 'image/heic',
    			'heif'                         => 'image/heif',
    			'heics'                        => 'image/heic-sequence',
    			'heifs'                        => 'image/heif-sequence',

    			// Video formats.
    			'asf|asx'                      => 'video/x-ms-asf',
    			'wmv'                          => 'video/x-ms-wmv',
    			'wmx'                          => 'video/x-ms-wmx',
    			'wm'                           => 'video/x-ms-wm',
    			'avi'                          => 'video/avi',
    			'divx'                         => 'video/divx',
    			'flv'                          => 'video/x-flv',
    			'mov|qt'                       => 'video/quicktime',
    			'mpeg|mpg|mpe'                 => 'video/mpeg',
    			'mp4|m4v'                      => 'video/mp4',
    			'ogv'                          => 'video/ogg',
    			'webm'                         => 'video/webm',
    			'mkv'                          => 'video/x-matroska',
    			'3gp|3gpp'                     => 'video/3gpp',  // Can also be audio.
    			'3g2|3gp2'                     => 'video/3gpp2', // Can also be audio.
    			// Text formats.
    			'txt|asc|c|cc|h|srt'           => 'text/plain',
    			'csv'                          => 'text/csv',
    			'tsv'                          => 'text/tab-separated-values',
    			'ics'                          => 'text/calendar',
    			'rtx'                          => 'text/richtext',
    			'css'                          => 'text/css',
    			'htm|html'                     => 'text/html',
    			'vtt'                          => 'text/vtt',
    			'dfxp'                         => 'application/ttaf+xml',
    			// Audio formats.
    			'mp3|m4a|m4b'                  => 'audio/mpeg',
    			'aac'                          => 'audio/aac',
    			'ra|ram'                       => 'audio/x-realaudio',
    			'wav|x-wav'                    => 'audio/wav',
    			'ogg|oga'                      => 'audio/ogg',
    			'flac'                         => 'audio/flac',
    			'mid|midi'                     => 'audio/midi',
    			'wma'                          => 'audio/x-ms-wma',
    			'wax'                          => 'audio/x-ms-wax',
    			'mka'                          => 'audio/x-matroska',
    			// Misc application formats.
    			'rtf'                          => 'application/rtf',
    			'js'                           => 'application/javascript',
    			'pdf'                          => 'application/pdf',
    			'swf'                          => 'application/x-shockwave-flash',
    			'class'                        => 'application/java',
    			'tar'                          => 'application/x-tar',
    			'zip'                          => 'application/zip',
    			'gz|gzip'                      => 'application/x-gzip',
    			'rar'                          => 'application/rar',
    			'7z'                           => 'application/x-7z-compressed',
    			'exe'                          => 'application/x-msdownload',
    			'psd'                          => 'application/octet-stream',
    			'xcf'                          => 'application/octet-stream',
    			// MS Office formats.
    			'doc'                          => 'application/msword',
    			'pot|pps|ppt'                  => 'application/vnd.ms-powerpoint',
    			'wri'                          => 'application/vnd.ms-write',
    			'xla|xls|xlt|xlw'              => 'application/vnd.ms-excel',
    			'mdb'                          => 'application/vnd.ms-access',
    			'mpp'                          => 'application/vnd.ms-project',
    			'docx'                         => 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
    			'docm'                         => 'application/vnd.ms-word.document.macroEnabled.12',
    			'dotx'                         => 'application/vnd.openxmlformats-officedocument.wordprocessingml.template',
    			'dotm'                         => 'application/vnd.ms-word.template.macroEnabled.12',
    			'xlsx'                         => 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
    			'xlsm'                         => 'application/vnd.ms-excel.sheet.macroEnabled.12',
    			'xlsb'                         => 'application/vnd.ms-excel.sheet.binary.macroEnabled.12',
    			'xltx'                         => 'application/vnd.openxmlformats-officedocument.spreadsheetml.template',
    			'xltm'                         => 'application/vnd.ms-excel.template.macroEnabled.12',
    			'xlam'                         => 'application/vnd.ms-excel.addin.macroEnabled.12',
    			'pptx'                         => 'application/vnd.openxmlformats-officedocument.presentationml.presentation',
    			'pptm'                         => 'application/vnd.ms-powerpoint.presentation.macroEnabled.12',
    			'ppsx'                         => 'application/vnd.openxmlformats-officedocument.presentationml.slideshow',
    			'ppsm'                         => 'application/vnd.ms-powerpoint.slideshow.macroEnabled.12',
    			'potx'                         => 'application/vnd.openxmlformats-officedocument.presentationml.template',
    			'potm'                         => 'application/vnd.ms-powerpoint.template.macroEnabled.12',
    			'ppam'                         => 'application/vnd.ms-powerpoint.addin.macroEnabled.12',
    			'sldx'                         => 'application/vnd.openxmlformats-officedocument.presentationml.slide',
    			'sldm'                         => 'application/vnd.ms-powerpoint.slide.macroEnabled.12',
    			'onetoc|onetoc2|onetmp|onepkg' => 'application/onenote',
    			'oxps'                         => 'application/oxps',
    			'xps'                          => 'application/vnd.ms-xpsdocument',
    			// OpenOffice formats.
    			'odt'                          => 'application/vnd.oasis.opendocument.text',
    			'odp'                          => 'application/vnd.oasis.opendocument.presentation',
    			'ods'                          => 'application/vnd.oasis.opendocument.spreadsheet',
    			'odg'                          => 'application/vnd.oasis.opendocument.graphics',
    			'odc'                          => 'application/vnd.oasis.opendocument.chart',
    			'odb'                          => 'application/vnd.oasis.opendocument.database',
    			'odf'                          => 'application/vnd.oasis.opendocument.formula',
    			// WordPerfect formats.
    			'wp|wpd'                       => 'application/wordperfect',
    			// iWork formats.
    			'key'                          => 'application/vnd.apple.keynote',
    			'numbers'                      => 'application/vnd.apple.numbers',
    			'pages'                        => 'application/vnd.apple.pages',
    		)
    	);
    }

    // Copied verbatim from WordPress 7.1.5's wp-includes/functions.php.
    /**
     * Retrieves the list of allowed mime types and file extensions.
     *
     * @since 2.8.6
     *
     * @param int|WP_User $user Optional. User to check. Defaults to current user.
     * @return string[] Array of mime types keyed by the file extension regex corresponding
     *                  to those types.
     */
    function get_allowed_mime_types( $user = null ) {
    	$t = wp_get_mime_types();

    	unset( $t['swf'], $t['exe'] );
    	if ( function_exists( 'current_user_can' ) ) {
    		$unfiltered = $user ? user_can( $user, 'unfiltered_html' ) : current_user_can( 'unfiltered_html' );
    	}

    	if ( empty( $unfiltered ) ) {
    		unset( $t['htm|html'], $t['js'] );
    	}

    	/**
    	 * Filters the list of allowed mime types and file extensions.
    	 *
    	 * @since 2.0.0
    	 *
    	 * @param array            $t    Mime types keyed by the file extension regex corresponding to those types.
    	 * @param int|WP_User|null $user User ID, User object or null if not provided (indicates current user).
    	 */
    	return apply_filters( 'upload_mimes', $t, $user );
    }

    // Copied verbatim from WordPress 7.1.5's wp-includes/functions.php.
    /**
     * Retrieves the file type from the file name.
     *
     * You can optionally define the mime array, if needed.
     *
     * @since 2.0.4
     *
     * @param string        $filename File name or path.
     * @param string[]|null $mimes    Optional. Array of allowed mime types keyed by their file extension regex.
     *                                Defaults to the result of get_allowed_mime_types().
     * @return array {
     *     Values for the extension and mime type.
     *
     *     @type string|false $ext  File extension, or false if the file doesn't match a mime type.
     *     @type string|false $type File mime type, or false if the file doesn't match a mime type.
     * }
     */
    function wp_check_filetype( $filename, $mimes = null ) {
    	if ( empty( $mimes ) ) {
    		$mimes = get_allowed_mime_types();
    	}
    	$type = false;
    	$ext  = false;

    	foreach ( $mimes as $ext_preg => $mime_match ) {
    		$ext_preg = '!\.(' . $ext_preg . ')$!i';
    		if ( preg_match( $ext_preg, $filename, $ext_matches ) ) {
    			$type = $mime_match;
    			$ext  = $ext_matches[1];
    			break;
    		}
    	}

    	return compact( 'ext', 'type' );
    }

    // Copied verbatim from WordPress 7.1.5's wp-includes/functions.php.
    /**
     * Converts smiley code to the icon graphic file equivalent.
     *
     * You can turn off smilies, by going to the write setting screen and unchecking
     * the box, or by setting 'use_smilies' option to false or removing the option.
     *
     * Plugins may override the default smiley list by setting the $wpsmiliestrans
     * to an array, with the key the code the blogger types in and the value the
     * image file.
     *
     * The $wp_smiliessearch global is for the regular expression and is set each
     * time the function is called.
     *
     * The full list of smilies can be found in the function and won't be listed in
     * the description. Probably should create a Codex page for it, so that it is
     * available.
     *
     * @since 2.2.0
     *
     * @global array $wpsmiliestrans
     * @global array $wp_smiliessearch
     */
    function smilies_init() {
    	global $wpsmiliestrans, $wp_smiliessearch;

    	// Don't bother setting up smilies if they are disabled.
    	if ( ! get_option( 'use_smilies' ) ) {
    		return;
    	}

    	if ( ! isset( $wpsmiliestrans ) ) {
    		$wpsmiliestrans = array(
    			':mrgreen:' => 'mrgreen.png',
    			':neutral:' => "\xf0\x9f\x98\x90",
    			':twisted:' => "\xf0\x9f\x98\x88",
    			':arrow:'   => "\xe2\x9e\xa1",
    			':shock:'   => "\xf0\x9f\x98\xaf",
    			':smile:'   => "\xf0\x9f\x99\x82",
    			':???:'     => "\xf0\x9f\x98\x95",
    			':cool:'    => "\xf0\x9f\x98\x8e",
    			':evil:'    => "\xf0\x9f\x91\xbf",
    			':grin:'    => "\xf0\x9f\x98\x80",
    			':idea:'    => "\xf0\x9f\x92\xa1",
    			':oops:'    => "\xf0\x9f\x98\xb3",
    			':razz:'    => "\xf0\x9f\x98\x9b",
    			':roll:'    => "\xf0\x9f\x99\x84",
    			':wink:'    => "\xf0\x9f\x98\x89",
    			':cry:'     => "\xf0\x9f\x98\xa5",
    			':eek:'     => "\xf0\x9f\x98\xae",
    			':lol:'     => "\xf0\x9f\x98\x86",
    			':mad:'     => "\xf0\x9f\x98\xa1",
    			':sad:'     => "\xf0\x9f\x99\x81",
    			'8-)'       => "\xf0\x9f\x98\x8e",
    			'8-O'       => "\xf0\x9f\x98\xaf",
    			':-('       => "\xf0\x9f\x99\x81",
    			':-)'       => "\xf0\x9f\x99\x82",
    			':-?'       => "\xf0\x9f\x98\x95",
    			':-D'       => "\xf0\x9f\x98\x80",
    			':-P'       => "\xf0\x9f\x98\x9b",
    			':-o'       => "\xf0\x9f\x98\xae",
    			':-x'       => "\xf0\x9f\x98\xa1",
    			':-|'       => "\xf0\x9f\x98\x90",
    			';-)'       => "\xf0\x9f\x98\x89",
    			// This one transformation breaks regular text with frequency.
    			//     '8)' => "\xf0\x9f\x98\x8e",
    			'8O'        => "\xf0\x9f\x98\xaf",
    			':('        => "\xf0\x9f\x99\x81",
    			':)'        => "\xf0\x9f\x99\x82",
    			':?'        => "\xf0\x9f\x98\x95",
    			':D'        => "\xf0\x9f\x98\x80",
    			':P'        => "\xf0\x9f\x98\x9b",
    			':o'        => "\xf0\x9f\x98\xae",
    			':x'        => "\xf0\x9f\x98\xa1",
    			':|'        => "\xf0\x9f\x98\x90",
    			';)'        => "\xf0\x9f\x98\x89",
    			':!:'       => "\xe2\x9d\x97",
    			':?:'       => "\xe2\x9d\x93",
    		);
    	}

    	/**
    	 * Filters all the smilies.
    	 *
    	 * This filter must be added before `smilies_init` is run, as
    	 * it is normally only run once to setup the smilies regex.
    	 *
    	 * @since 4.7.0
    	 *
    	 * @param string[] $wpsmiliestrans List of the smilies' hexadecimal representations, keyed by their smily code.
    	 */
    	$wpsmiliestrans = apply_filters( 'smilies', $wpsmiliestrans );

    	if ( count( $wpsmiliestrans ) === 0 ) {
    		return;
    	}

    	/*
    	 * NOTE: we sort the smilies in reverse key order. This is to make sure
    	 * we match the longest possible smilie (:???: vs :?) as the regular
    	 * expression used below is first-match
    	 */
    	krsort( $wpsmiliestrans );

    	$spaces = wp_spaces_regexp();

    	// Begin first "subpattern".
    	$wp_smiliessearch = '/(?<=' . $spaces . '|^)';

    	$subchar = '';
    	foreach ( (array) $wpsmiliestrans as $smiley => $img ) {
    		$firstchar = substr( $smiley, 0, 1 );
    		$rest      = substr( $smiley, 1 );

    		// New subpattern?
    		if ( $firstchar !== $subchar ) {
    			if ( '' !== $subchar ) {
    				$wp_smiliessearch .= ')(?=' . $spaces . '|$)';  // End previous "subpattern".
    				$wp_smiliessearch .= '|(?<=' . $spaces . '|^)'; // Begin another "subpattern".
    			}

    			$subchar           = $firstchar;
    			$wp_smiliessearch .= preg_quote( $firstchar, '/' ) . '(?:';
    		} else {
    			$wp_smiliessearch .= '|';
    		}

    		$wp_smiliessearch .= preg_quote( $rest, '/' );
    	}

    	$wp_smiliessearch .= ')(?=' . $spaces . '|$)/m';
    }

    // make_absolute_url() copied verbatim from WordPress 7.1.5's wp-includes/class-wp-http.php; the real class
    // file needs the Requests autoloader, so this stand-in class holds only that method.
    class WP_Http
    {
    /**
    	 * Converts a relative URL to an absolute URL relative to a given URL.
    	 *
    	 * If an Absolute URL is provided, no processing of that URL is done.
    	 *
    	 * @since 3.4.0
    	 *
    	 * @param string $maybe_relative_path The URL which might be relative.
    	 * @param string $url                 The URL which $maybe_relative_path is relative to.
    	 * @return string An Absolute URL, in a failure condition where the URL cannot be parsed, the relative URL will be returned.
    	 */
    	public static function make_absolute_url( $maybe_relative_path, $url ) {
    		if ( empty( $url ) ) {
    			return $maybe_relative_path;
    		}

    		$url_parts = wp_parse_url( $url );
    		if ( ! $url_parts ) {
    			return $maybe_relative_path;
    		}

    		$relative_url_parts = wp_parse_url( $maybe_relative_path );
    		if ( ! $relative_url_parts ) {
    			return $maybe_relative_path;
    		}

    		// Check for a scheme on the 'relative' URL.
    		if ( ! empty( $relative_url_parts['scheme'] ) ) {
    			return $maybe_relative_path;
    		}

    		$absolute_path = $url_parts['scheme'] . '://';

    		// Schemeless URLs will make it this far, so we check for a host in the relative URL
    		// and convert it to a protocol-URL.
    		if ( isset( $relative_url_parts['host'] ) ) {
    			$absolute_path .= $relative_url_parts['host'];
    			if ( isset( $relative_url_parts['port'] ) ) {
    				$absolute_path .= ':' . $relative_url_parts['port'];
    			}
    		} else {
    			$absolute_path .= $url_parts['host'];
    			if ( isset( $url_parts['port'] ) ) {
    				$absolute_path .= ':' . $url_parts['port'];
    			}
    		}

    		// Start off with the absolute URL path.
    		$path = ! empty( $url_parts['path'] ) ? $url_parts['path'] : '/';

    		// If it's a root-relative path, then great.
    		if ( ! empty( $relative_url_parts['path'] ) && '/' === $relative_url_parts['path'][0] ) {
    			$path = $relative_url_parts['path'];

    			// Else it's a relative path.
    		} elseif ( ! empty( $relative_url_parts['path'] ) ) {
    			// Strip off any file components from the absolute path.
    			$path = substr( $path, 0, strrpos( $path, '/' ) + 1 );

    			// Build the new path.
    			$path .= $relative_url_parts['path'];

    			// Strip all /path/../ out of the path.
    			while ( strpos( $path, '../' ) > 1 ) {
    				$path = preg_replace( '![^/]+/\.\./!', '', $path, -1, $segment_replacement_count );
    				if ( 0 === $segment_replacement_count ) {
    					break;
    				}
    			}

    			// Strip any final leading ../ from the path.
    			$path = preg_replace( '!^/(\.\./)+!', '', $path );
    		}

    		// Add the query string.
    		if ( ! empty( $relative_url_parts['query'] ) ) {
    			$path .= '?' . $relative_url_parts['query'];
    		}

    		// Add the fragment.
    		if ( ! empty( $relative_url_parts['fragment'] ) ) {
    			$path .= '#' . $relative_url_parts['fragment'];
    		}

    		return $absolute_path . '/' . ltrim( $path, '/' );
    	}
    }


    // Copied verbatim from WordPress 7.1.5's wp-includes/functions.php.
    /**
     * Retrieves a list of protocols to allow in HTML attributes.
     *
     * @since 3.3.0
     * @since 4.3.0 Added 'webcal' to the protocols array.
     * @since 4.7.0 Added 'urn' to the protocols array.
     * @since 5.3.0 Added 'sms' to the protocols array.
     * @since 5.6.0 Added 'irc6' and 'ircs' to the protocols array.
     *
     * @see wp_kses()
     * @see esc_url()
     *
     * @return string[] Array of allowed protocols. Defaults to an array containing 'http', 'https',
     *                  'ftp', 'ftps', 'mailto', 'news', 'irc', 'irc6', 'ircs', 'gopher', 'nntp', 'feed',
     *                  'telnet', 'mms', 'rtsp', 'sms', 'svn', 'tel', 'fax', 'xmpp', 'webcal', and 'urn'.
     *                  This covers all common link protocols, except for 'javascript' which should not
     *                  be allowed for untrusted users.
     */
    if (!function_exists('wp_allowed_protocols')) {
    function wp_allowed_protocols() {
    	static $protocols = array();

    	if ( empty( $protocols ) ) {
    		$protocols = array( 'http', 'https', 'ftp', 'ftps', 'mailto', 'news', 'irc', 'irc6', 'ircs', 'gopher', 'nntp', 'feed', 'telnet', 'mms', 'rtsp', 'sms', 'svn', 'tel', 'fax', 'xmpp', 'webcal', 'urn' );
    	}

    	if ( ! did_action( 'wp_loaded' ) ) {
    		/**
    		 * Filters the list of protocols allowed in HTML attributes.
    		 *
    		 * @since 3.0.0
    		 *
    		 * @param string[] $protocols Array of allowed protocols e.g. 'http', 'ftp', 'tel', and more.
    		 */
    		$protocols = array_unique( (array) apply_filters( 'kses_allowed_protocols', $protocols ) );
    	}

    	return $protocols;
    }
    } // function_exists( 'wp_allowed_protocols' ): kses-php declares it natively on the port's side.

    // Copied verbatim from WordPress 7.1.5's wp-includes/functions.php.
    /**
     * Checks value to find if it was serialized.
     *
     * If $data is not a string, then returned value will always be false.
     * Serialized data is always a string.
     *
     * @since 2.0.5
     * @since 6.1.0 Added Enum support.
     *
     * @param string $data   Value to check to see if was serialized.
     * @param bool   $strict Optional. Whether to be strict about the end of the string. Default true.
     * @return bool False if not serialized and true if it was.
     */
    function is_serialized( $data, $strict = true ) {
    	// If it isn't a string, it isn't serialized.
    	if ( ! is_string( $data ) ) {
    		return false;
    	}
    	$data = trim( $data );
    	if ( 'N;' === $data ) {
    		return true;
    	}
    	if ( strlen( $data ) < 4 ) {
    		return false;
    	}
    	if ( ':' !== $data[1] ) {
    		return false;
    	}
    	if ( $strict ) {
    		$lastc = substr( $data, -1 );
    		if ( ';' !== $lastc && '}' !== $lastc ) {
    			return false;
    		}
    	} else {
    		$semicolon = strpos( $data, ';' );
    		$brace     = strpos( $data, '}' );
    		// Either ; or } must exist.
    		if ( false === $semicolon && false === $brace ) {
    			return false;
    		}
    		// But neither must be in the first X characters.
    		if ( false !== $semicolon && $semicolon < 3 ) {
    			return false;
    		}
    		if ( false !== $brace && $brace < 4 ) {
    			return false;
    		}
    	}
    	$token = $data[0];
    	switch ( $token ) {
    		case 's':
    			if ( $strict ) {
    				if ( '"' !== substr( $data, -2, 1 ) ) {
    					return false;
    				}
    			} elseif ( ! str_contains( $data, '"' ) ) {
    				return false;
    			}
    			// Or else fall through.
    		case 'a':
    		case 'O':
    		case 'E':
    			return (bool) preg_match( "/^{$token}:[0-9]+:/s", $data );
    		case 'b':
    		case 'i':
    		case 'd':
    			$end = $strict ? '$' : '';
    			return (bool) preg_match( "/^{$token}:[0-9.E+-]+;$end/", $data );
    	}
    	return false;
    }

    smilies_init();

    /**
     * $wp_smiliessearch as smilies_init() built it, so the Rust port's pattern builder can be checked against it.
     *
     * @return string
     *     The search pattern.
     */
    function speedyspec_smilies_search()
    {
        return $GLOBALS['wp_smiliessearch'];
    }

    /**
     * convert_smilies() with $wpsmiliestrans set to $table, as convertSmilies.php's custom-table tests run it.
     *
     * @param string $text
     *     The text.
     * @param array $table
     *     The smiley table.
     * @return string
     *     The converted text.
     */
    function speedyspec_convert_smilies_with($text, $table)
    {
        global $wpsmiliestrans;
        $saved = $wpsmiliestrans;
        $wpsmiliestrans = $table;
        smilies_init();
        try {
            return convert_smilies($text);
        } finally {
            $wpsmiliestrans = $saved;
            smilies_init();
        }
    }

    /**
     * antispambot() after mt_srand($seed), so its rand() choices are reproducible.
     *
     * @param string $email
     *     The address.
     * @param int $hex
     *     antispambot()'s $hex_encoding.
     * @param int $seed
     *     The seed.
     * @return string
     *     The obfuscated address.
     */
    function speedyspec_antispambot_seeded($email, $hex, $seed)
    {
        mt_srand($seed);
        return antispambot($email, $hex);
    }

    /**
     * make_clickable() while comment_text is the current filter, as makeClickable.php's test_add_rel_ugc_in_comments
     * reaches it through comment_text().
     *
     * @param string $text
     *     The text.
     * @return string
     *     The linked text.
     */
    function speedyspec_make_clickable_in_comment_text($text)
    {
        // Run it as a comment_text callback (priority 9, as default-filters.php has it), so current_filter() answers
        // comment_text with WordPress's hooks or native ones.
        add_filter('comment_text', 'make_clickable', 9);
        try {
            return apply_filters('comment_text', $text);
        } finally {
            remove_filter('comment_text', 'make_clickable', 9);
        }
    }
    /**
     * wp_parse_str()'s result, which it hands back by reference.
     *
     * @param mixed $input
     *     The query string.
     * @return array
     *     The parsed variables, after the wp_parse_str filter.
     */
    function speedyspec_wp_parse_str($input)
    {
        wp_parse_str($input, $result);
        return $result;
    }
}
