<?php

declare(strict_types=1);

/**
 * Sets up the theme.json scenarios the same way on both sides.
 *
 *  `SPEEDYSPEC_COMPAT_WP_ROOT` is the release to load from and `SPEEDYSPEC_COMPAT_PORT` whether the native port is
 *  loaded. The recorder loads WordPress's `plugin.php` and its theme.json classes; the port's side loads neither
 *  (they are native). Both load the block registries and the formatting functions theme.json reaches, and the same
 *  stubs, so only the theme.json engine differs between the two runs.
 *
 *  Array results go through {@link compat_canon()}, so the fixture compares them as PHP's `==` does (the theme.json
 *  design's "same contents, schema order" rule). CSS strings are compared byte for byte.
 */

$GLOBALS['speedyspec_notices'] = [];

if (!defined('ABSPATH')) {
    define('ABSPATH', SPEEDYSPEC_COMPAT_WP_ROOT);
}
if (!defined('WPINC')) {
    define('WPINC', 'wp-includes');
}

if (!function_exists('__')) {
    function __($text, $domain = 'default') { return $text; }
}
if (!function_exists('_x')) {
    function _x($text, $context, $domain = 'default') { return $text; }
}
if (!function_exists('_doing_it_wrong')) {
    function _doing_it_wrong($function_name, $message, $version) {
        $GLOBALS['speedyspec_notices'][] = "doing_it_wrong:$function_name|$message|$version";
    }
}
if (!function_exists('_deprecated_function')) {
    function _deprecated_function($function_name, $version, $replacement = '') {
        $GLOBALS['speedyspec_notices'][] = "deprecated_function:$function_name|$version|$replacement";
    }
}
if (!function_exists('_deprecated_argument')) {
    function _deprecated_argument($function_name, $version, $message = '') {
        $GLOBALS['speedyspec_notices'][] = "deprecated_argument:$function_name|$version|$message";
    }
}
if (!function_exists('wp_trigger_error')) {
    function wp_trigger_error($function_name, $message, $error_level = E_USER_NOTICE) {
        $GLOBALS['speedyspec_notices'][] = "trigger_error:$function_name|$message|$error_level";
    }
}
if (!function_exists('get_option')) {
    function get_option($option, $default_value = false) { return 'blog_charset' === $option ? 'UTF-8' : $default_value; }
}
if (!function_exists('is_utf8_charset')) {
    function is_utf8_charset($blog_charset = null) { return true; }
}
if (!function_exists('wp_allowed_protocols')) {
    function wp_allowed_protocols() {
        return ['http', 'https', 'ftp', 'ftps', 'mailto', 'news', 'irc', 'irc6', 'ircs', 'gopher', 'nntp', 'feed',
            'telnet', 'mms', 'rtsp', 'sms', 'svn', 'tel', 'fax', 'xmpp', 'webcal', 'urn'];
    }
}

if (!SPEEDYSPEC_COMPAT_PORT) {
    require SPEEDYSPEC_COMPAT_WP_ROOT . 'wp-includes/plugin.php';
}
foreach (['utf8.php', 'compat-utf8.php', 'class-wp-token-map.php', 'html-api/html5-named-character-references.php',
    'html-api/class-wp-html-decoder.php', 'formatting.php', 'kses.php', 'class-wp-block-type.php',
    'class-wp-block-type-registry.php', 'class-wp-block-styles-registry.php'] as $speedyspec_file) {
    if (is_file(SPEEDYSPEC_COMPAT_WP_ROOT . 'wp-includes/' . $speedyspec_file)) {
        require_once SPEEDYSPEC_COMPAT_WP_ROOT . 'wp-includes/' . $speedyspec_file;
    }
}
if (!function_exists('wp_is_valid_utf8')) {
    function wp_is_valid_utf8($bytes) { return mb_check_encoding($bytes, 'UTF-8'); }
}

/**
 * Defines only the named functions from a release file, so a file that also defines functions this area stubs or
 *  ports can still lend the rest.
 */
function compat_load_functions(string $file, array $names): void
{
    $tokens = token_get_all(file_get_contents(SPEEDYSPEC_COMPAT_WP_ROOT . $file));
    $count = count($tokens);
    for ($i = 0; $i < $count; $i++) {
        if (!is_array($tokens[$i]) || T_FUNCTION !== $tokens[$i][0]) {
            continue;
        }
        $j = $i + 1;
        while ($j < $count && is_array($tokens[$j]) && T_WHITESPACE === $tokens[$j][0]) {
            $j++;
        }
        if (!is_array($tokens[$j]) || T_STRING !== $tokens[$j][0] || !in_array($tokens[$j][1], $names, true)
            || function_exists($tokens[$j][1])) {
            continue;
        }
        $source = '';
        $depth = 0;
        $opened = false;
        for ($k = $i; $k < $count; $k++) {
            $text = is_array($tokens[$k]) ? $tokens[$k][1] : $tokens[$k];
            $source .= $text;
            $opens = '{' === $text
                || (is_array($tokens[$k]) && in_array($tokens[$k][0], [T_CURLY_OPEN, T_DOLLAR_OPEN_CURLY_BRACES], true));
            if ($opens) {
                $depth++;
                $opened = true;
            } elseif ('}' === $text) {
                $depth--;
                if ($opened && 0 === $depth) {
                    break;
                }
            }
        }
        eval($source);
        $i = $k;
    }
}

// WordPress's default filter (default-filters.php), so titles are sanitized as on a real site.
add_filter('sanitize_title', 'sanitize_title_with_dashes', 10, 3);

compat_load_functions('wp-includes/functions.php', ['_wp_array_get', '_wp_array_set', 'wp_is_numeric_array',
    '_wp_to_kebab_case', 'wp_recursive_ksort', 'wp_parse_args', 'wp_json_file_decode', 'mbstring_binary_safe_encoding',
    'reset_mbstring_encoding', '_canonical_charset', 'wp_json_encode', '_wp_json_sanity_check', '_wp_json_convert_string']);

if (!SPEEDYSPEC_COMPAT_PORT) {
    foreach (['class-wp-theme-json-schema.php', 'class-wp-theme-json.php', 'class-wp-theme-json-data.php'] as $speedyspec_file) {
        require_once SPEEDYSPEC_COMPAT_WP_ROOT . 'wp-includes/' . $speedyspec_file;
    }
    compat_load_functions('wp-includes/global-styles-and-settings.php', ['wp_get_block_css_selector']);
    // What the stylesheet code reaches outside the class: fluid typography, the layout definitions, block
    // supports, PHP 8.6's clamp() polyfill and the style engine (background images).
    compat_load_functions('wp-includes/compat.php', ['clamp']);
    compat_load_functions('wp-includes/block-supports/typography.php', ['wp_get_typography_value_and_unit',
        'wp_get_computed_fluid_typography_value', 'wp_get_typography_font_size_value']);
    compat_load_functions('wp-includes/block-supports/layout.php', ['wp_get_layout_definitions']);
    compat_load_functions('wp-includes/blocks.php', ['block_has_support']);
    // get_svg_filters() calls WP_Duotone, which the block supports area owns.
    require_once SPEEDYSPEC_COMPAT_WP_ROOT . 'wp-includes/class-wp-duotone.php';
    foreach (['style-engine.php', 'style-engine/class-wp-style-engine.php',
        'style-engine/class-wp-style-engine-css-declarations.php', 'style-engine/class-wp-style-engine-css-rule.php',
        'style-engine/class-wp-style-engine-css-rules-store.php', 'style-engine/class-wp-style-engine-processor.php'] as $speedyspec_file) {
        require_once SPEEDYSPEC_COMPAT_WP_ROOT . 'wp-includes/' . $speedyspec_file;
    }
}

/**
 * The site's settings fluid typography reads come from the resolver on both sides (WordPress's on the recorder, the
 * port's on the other); a scenario changes them with a user-layer filter, the same code on both sides.
 */
function compat_set_global_settings(array $settings): void
{
    add_filter('wp_theme_json_data_user', static function ($data) use ($settings) {
        return $data->update_with(array('version' => 3, 'settings' => $settings));
    });
    wp_clean_theme_json_cache();
}

/** WordPress's object cache and transients, over arrays. */
if (!function_exists('wp_cache_get')) {
    function wp_cache_get($key, $group = '', $force = false, &$found = null)
    {
        $found = isset($GLOBALS['compat_cache'][$group]) && array_key_exists($key, $GLOBALS['compat_cache'][$group]);
        return $found ? $GLOBALS['compat_cache'][$group][$key] : false;
    }
    function wp_cache_set($key, $data, $group = '', $expire = 0) { $GLOBALS['compat_cache'][$group][$key] = $data; return true; }
    function wp_cache_delete($key, $group = '') { unset($GLOBALS['compat_cache'][$group][$key]); return true; }
}
if (!function_exists('get_transient')) {
    function get_transient($transient) { return $GLOBALS['compat_transients'][$transient] ?? false; }
    function set_transient($transient, $value, $expiration = 0) { $GLOBALS['compat_transients'][$transient] = $value; return true; }
}

/** `current_user_can()`, answering from compat_set_caps(): a request without a user can do nothing. */
if (!function_exists('current_user_can')) {
    function current_user_can($capability, ...$args) {
        return in_array($capability, $GLOBALS['compat_caps'] ?? array(), true);
    }
}

function compat_set_caps(array $caps): void
{
    $GLOBALS['compat_caps'] = $caps;
}

if (!defined('SCRIPT_DEBUG')) {
    define('SCRIPT_DEBUG', false);
}

/** `current_theme_supports()`, answering from compat_add_theme_support(). */
if (!function_exists('current_theme_supports')) {
    function current_theme_supports($feature, ...$args) {
        return in_array($feature, $GLOBALS['compat_theme_supports'] ?? array(), true);
    }
}

function compat_add_theme_support(string $feature): void
{
    $GLOBALS['compat_theme_supports'][] = $feature;
}

/**
 * Called by the driver once it is loaded: reports the notices after each step.
 */
function speedyspec_compat_area_setup()
{
    SpeedySpecCompat::$report_globals = false;
    SpeedySpecCompat::$state = fn() => ['notices' => $GLOBALS['speedyspec_notices']];
}

/**
 * Sorts every array that is not a list by key, recursively, so the fixture compares as PHP's `==` does.
 */
function compat_canon(mixed $value): mixed
{
    if (!is_array($value)) {
        return $value;
    }
    foreach ($value as $k => $v) {
        $value[$k] = compat_canon($v);
    }
    if (!array_is_list($value)) {
        ksort($value, SORT_STRING);
    }
    return $value;
}

/** A theme.json document, built as WordPress builds one. */
function compat_theme_json(array $data = ['version' => 3], string $origin = 'theme'): WP_Theme_JSON
{
    return new WP_Theme_JSON($data, $origin);
}

/** Registers a block type, unregistering any earlier one of the same name. */
function compat_register_block(string $name, array $args = []): void
{
    $registry = WP_Block_Type_Registry::get_instance();
    if ($registry->is_registered($name)) {
        $registry->unregister($name);
    }
    $registry->register($name, $args);
}

/** Registers a block style for a block. */
function compat_register_block_style(string $block, array $style): void
{
    WP_Block_Styles_Registry::get_instance()->register($block, $style);
}

/** A `WP_Theme_JSON` class constant, canonicalised. */
function compat_constant(string $name): mixed
{
    return compat_canon(constant("WP_Theme_JSON::$name"));
}

/** `WP_Theme_JSON_Schema::migrate()`, canonicalised. */
function compat_migrate(mixed $data, string $origin = 'theme'): mixed
{
    return compat_canon(WP_Theme_JSON_Schema::migrate($data, $origin));
}

/** Runs a PHP snippet that returns a value, canonicalised; the scenarios' cases are small snippets. */
function compat_eval(string $code): mixed
{
    return compat_canon(eval($code));
}

/**
 * Registers every block the release ships (`wp-includes/blocks/<name>/block.json`): name, styles, supports and
 *  selectors, which is all the theme.json engine reads.
 */
function compat_register_core_blocks(): void
{
    foreach (glob(SPEEDYSPEC_COMPAT_WP_ROOT . 'wp-includes/blocks/*/block.json') as $file) {
        $meta = json_decode(file_get_contents($file), true);
        if (!isset($meta['name'])) {
            continue;
        }
        $args = array();
        foreach (array('styles', 'supports', 'selectors') as $key) {
            if (isset($meta[$key])) {
                $args[$key] = $meta[$key];
            }
        }
        compat_register_block($meta['name'], $args);
    }
}

/** A JSON file of the release, decoded. */
function compat_release_json(string $path): mixed
{
    return json_decode(file_get_contents(SPEEDYSPEC_COMPAT_WP_ROOT . $path), true);
}

/*
 * The theme environment the resolver and the global styles functions read, the same on both sides: a theme stub over
 * the directories compat_switch_theme() sets, theme supports, the user's global styles post, and the object cache.
 */

/** A minimal `WP_Theme` over a theme directory: its `style.css` headers, `get_file_path()` and its parent. */
class Compat_Theme
{
    public function __construct(public string $dir, public ?string $template_dir = null) {}

    private function header(string $name): string|false
    {
        $css = @file_get_contents($this->dir . '/style.css');
        if (false !== $css && preg_match('/^[ \t\/*#@]*' . preg_quote($name, '/') . ':(.*)$/mi', $css, $m)) {
            return trim(preg_replace('/\s*(?:\*\/|\?>).*/', '', $m[1]));
        }
        return false;
    }

    public function get($header) { return $this->header($header); }
    public function get_stylesheet() { return basename($this->dir); }
    public function get_stylesheet_directory() { return $this->dir; }
    public function get_template_directory() { return $this->template_dir ?? $this->dir; }
    public function parent() { return null === $this->template_dir ? false : new Compat_Theme($this->template_dir); }
    public function get_file_path($file = '')
    {
        $file = ltrim($file, '/');
        $stylesheet = $this->get_stylesheet_directory();
        $template = $this->get_template_directory();
        if (empty($file)) {
            $path = $stylesheet;
        } elseif ($stylesheet !== $template && file_exists($stylesheet . '/' . $file)) {
            $path = $stylesheet . '/' . $file;
        } else {
            $path = $template . '/' . $file;
        }
        return apply_filters('theme_file_path', $path, $file);
    }
}

/** Activates the theme in `$dir` (with the parent in `$template_dir`). */
function compat_switch_theme(string $dir, ?string $template_dir = null): void
{
    $GLOBALS['compat_theme'] = new Compat_Theme($dir, $template_dir);
}

if (!function_exists('wp_get_theme')) {
    function wp_get_theme($stylesheet = '', $theme_root = '') { return $GLOBALS['compat_theme'] ?? new Compat_Theme(''); }
}
if (!function_exists('get_stylesheet')) {
    function get_stylesheet() { return wp_get_theme()->get_stylesheet(); }
}
if (!function_exists('get_stylesheet_directory')) {
    function get_stylesheet_directory() { return wp_get_theme()->get_stylesheet_directory(); }
}
if (!function_exists('get_template_directory')) {
    function get_template_directory() { return wp_get_theme()->get_template_directory(); }
}
if (!function_exists('wp_is_development_mode')) {
    function wp_is_development_mode($mode) { return in_array($mode, $GLOBALS['compat_development_modes'] ?? array(), true); }
}
if (!function_exists('wp_is_block_theme')) {
    function wp_is_block_theme() { return file_exists(get_template_directory() . '/templates/index.html') || file_exists(get_template_directory() . '/block-templates/index.html'); }
}
if (!function_exists('get_theme_support')) {
    function get_theme_support($feature, ...$args) { return $GLOBALS['compat_theme_support_args'][$feature] ?? false; }
}
if (!function_exists('translate_with_gettext_context')) {
    function translate_with_gettext_context($text, $context, $domain = 'default') { return $text; }
}
if (!function_exists('get_theme_file_uri')) {
    function get_theme_file_uri($file = '')
    {
        $url = 'https://example.org/wp-content/themes/' . get_stylesheet() . '/' . ltrim($file, '/');
        return apply_filters('theme_file_uri', $url, ltrim($file, '/'));
    }
}
if (!function_exists('wp_check_filetype')) {
    function wp_check_filetype($filename, $mimes = null)
    {
        $types = array('jpg|jpeg|jpe' => 'image/jpeg', 'png' => 'image/png', 'gif' => 'image/gif', 'webp' => 'image/webp', 'avif' => 'image/avif');
        foreach ($types as $exts => $mime) {
            if (preg_match('!\.(' . $exts . ')$!i', $filename, $m)) {
                return array('ext' => $m[1], 'type' => $mime);
            }
        }
        return array('ext' => false, 'type' => false);
    }
}
if (!function_exists('is_wp_error')) {
    function is_wp_error($thing) { return false; }
}

/** Gives the theme a feature with arguments, as `add_theme_support( $feature, $args )` would. */
function compat_add_theme_support_args(string $feature, $args): void
{
    $GLOBALS['compat_theme_support_args'][$feature] = array($args);
    $GLOBALS['compat_theme_supports'][] = $feature;
}

/** The user's global styles posts, by theme stylesheet. */
if (!class_exists('WP_Post', false)) {
    #[AllowDynamicProperties]
    class WP_Post
    {
        public $ID; public $post_content = ''; public $post_status = 'publish'; public $post_type = 'wp_global_styles';
        public $post_title = ''; public $post_name = '';
        public function __construct(array $fields) { foreach ($fields as $k => $v) { $this->$k = $v; } }
    }
}
if (!class_exists('WP_Query', false)) {
    class WP_Query
    {
        public function query($args)
        {
            $stylesheet = $args['tax_query'][0]['terms'] ?? '';
            $statuses = (array) ($args['post_status'] ?? array('publish'));
            foreach (array_reverse($GLOBALS['compat_posts'] ?? array()) as $post) {
                if (($post['stylesheet'] ?? '') === $stylesheet && in_array($post['post_status'] ?? 'publish', $statuses, true)) {
                    return array(new WP_Post(array_diff_key($post, array('stylesheet' => 1))));
                }
            }
            return array();
        }
    }
}
if (!function_exists('wp_insert_post')) {
    function wp_insert_post($postarr, $wp_error = false)
    {
        $id = 100 + count($GLOBALS['compat_posts'] ?? array());
        $GLOBALS['compat_posts'][] = array('ID' => $id, 'stylesheet' => $postarr['tax_input']['wp_theme'][0] ?? '') + $postarr;
        return $id;
    }
}
if (!function_exists('get_post')) {
    function get_post($id)
    {
        foreach ($GLOBALS['compat_posts'] ?? array() as $post) {
            if (($post['ID'] ?? null) === $id) {
                return new WP_Post(array_diff_key($post, array('stylesheet' => 1)));
            }
        }
        return null;
    }
}

/** Saves the user's global styles for a theme. */
function compat_add_user_styles(string $stylesheet, array $data, string $status = 'publish'): void
{
    $id = 100 + count($GLOBALS['compat_posts'] ?? array());
    $GLOBALS['compat_posts'][] = array('ID' => $id, 'stylesheet' => $stylesheet, 'post_status' => $status,
        'post_content' => json_encode($data), 'post_type' => 'wp_global_styles');
}

/** Loads WordPress's resolver and what it calls (the recorder; the port defines its own). */
function compat_load_resolver(): void
{
    compat_load_functions('wp-includes/functions.php', ['wp_json_file_decode', 'wp_normalize_path', 'wp_is_stream']);
    compat_load_functions('wp-includes/l10n.php', ['translate_settings_using_i18n_schema']);
    compat_load_functions('wp-includes/block-editor.php', ['get_classic_theme_supports_block_editor_settings']);
    compat_load_functions('wp-includes/block-supports/block-style-variations.php', ['wp_register_block_style_variations_from_theme_json_partials']);
    compat_load_functions('wp-includes/blocks.php', ['register_block_style']);
    if (!SPEEDYSPEC_COMPAT_PORT) {
        compat_load_functions('wp-includes/global-styles-and-settings.php', ['wp_get_global_settings',
            'wp_get_global_styles', 'wp_get_global_stylesheet', 'wp_add_global_styles_for_blocks',
            'wp_get_block_name_from_theme_json_path', 'wp_theme_has_theme_json', 'wp_clean_theme_json_cache',
            'wp_get_theme_directory_pattern_slugs', 'wp_get_theme_data_custom_templates',
            'wp_get_theme_data_template_parts']);
        require_once SPEEDYSPEC_COMPAT_WP_ROOT . 'wp-includes/class-wp-theme-json-resolver.php';
    }
}
compat_load_resolver();

/** A test theme shipped with the engine's tests (`tests/data/themedir1/<name>`). */
function compat_theme_dir(string $name): string
{
    return dirname(__DIR__, 2) . '/crates/speedyspec-wp-theme-json/tests/data/themedir1/' . $name;
}

/** An array's keys, sorted (the port promises equal arrays by `==`, not the same key order). */
function compat_sorted_keys($array): array
{
    $keys = array_keys((array) $array);
    sort($keys, SORT_STRING);
    return $keys;
}
