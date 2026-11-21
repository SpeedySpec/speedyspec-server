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

    require ABSPATH . WPINC . '/plugin.php';
    require ABSPATH . WPINC . '/kses.php';
    require ABSPATH . WPINC . '/formatting.php';
    require ABSPATH . WPINC . '/shortcodes.php';

    // Formatting's default callbacks on the hooks its own functions fire, copied from WordPress 7.1.5's
    // wp-includes/default-filters.php. The rest of formatting's share of that file is on hooks other seams fire;
    // `register_text_filters` and its tests cover those.
    add_filter( 'sanitize_title', 'sanitize_title_with_dashes', 10, 3 );
}
