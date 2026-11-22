// Not strict: WordPress's class-wp-theme-json.php coerces arguments, and so must this.

namespace SpeedySpec\WpThemeJson\Support
{
    /**
     * Lists the registered blocks for the Rust engine: name, selectors, supports and style variation names.
     *
     *  Style names come from `block.json` first, then from the block styles registry, as WordPress collects them.
     */
    function registered_blocks()
    {
        if (!class_exists('WP_Block_Type_Registry', false)) {
            return array();
        }
        $styles = class_exists('WP_Block_Styles_Registry', false) ? \WP_Block_Styles_Registry::get_instance() : null;
        $out = array();
        foreach (\WP_Block_Type_Registry::get_instance()->get_all_registered() as $name => $type) {
            $names = array();
            foreach ((array) ($type->styles ?? array()) as $style) {
                if (isset($style['name'])) {
                    $names[$style['name']] = true;
                }
            }
            if (null !== $styles) {
                foreach ($styles->get_registered_styles_for_block($name) as $style) {
                    $names[$style['name']] = true;
                }
            }
            $out[] = array($name, $type->selectors ?? array(), $type->supports ?? array(), array_keys($names));
        }
        return $out;
    }

    /**
     * The block registered under this name right now, as `registered_blocks()` lists one, or null.
     */
    function registered_block($name)
    {
        if (!class_exists('WP_Block_Type_Registry', false)) {
            return null;
        }
        $type = \WP_Block_Type_Registry::get_instance()->get_registered($name);
        if (null === $type) {
            return null;
        }
        return array($name, $type->selectors ?? array(), $type->supports ?? array(), array());
    }

    /**
     * Fingerprints the registered block and block style names, so the engine re-reads the registries only after
     *  something is registered or unregistered.
     */
    function block_marker()
    {
        if (!class_exists('WP_Block_Type_Registry', false)) {
            return '';
        }
        $names = array_keys(\WP_Block_Type_Registry::get_instance()->get_all_registered());
        if (class_exists('WP_Block_Styles_Registry', false)) {
            foreach (\WP_Block_Styles_Registry::get_instance()->get_all_registered() as $block => $styles) {
                $names[] = $block . '|' . implode('|', array_keys((array) $styles));
            }
        }
        return md5(implode("\n", $names));
    }
    /** `wp_get_theme()->get_file_path( 'theme.json' )`. */
    function theme_json_path()
    {
        return wp_get_theme()->get_file_path('theme.json');
    }

    /** The parent theme's `theme.json` path, or null without a parent. */
    function parent_theme_json_path()
    {
        $parent = wp_get_theme()->parent();
        return $parent ? $parent->get_file_path('theme.json') : null;
    }

    /** The theme's (or its parent's) `TextDomain` header. */
    function text_domain($parent)
    {
        $theme = wp_get_theme();
        if ($parent) {
            $theme = $theme->parent();
        }
        return $theme ? $theme->get('TextDomain') : false;
    }

    /** Where core's `theme.json` lives. */
    function core_directory()
    {
        return rtrim(ABSPATH . WPINC, '/');
    }

    /** `WP_Block_Styles_Registry::get_instance()->get_all_registered()`. */
    function registered_styles()
    {
        return class_exists('WP_Block_Styles_Registry', false) ? \WP_Block_Styles_Registry::get_instance()->get_all_registered() : array();
    }

    /**
     * Applies a `wp_theme_json_data_*` filter to a layer wrapped in `WP_Theme_JSON_Data`; returns what the resolver
     *  needs from the result: the built document of a `WP_Theme_JSON_Data`, or another object's `get_data()`.
     */
    function apply_data_filter($hook, $config, $origin)
    {
        $result = apply_filters($hook, new \WP_Theme_JSON_Data($config, $origin));
        if ($result instanceof \WP_Theme_JSON_Data) {
            return array('built', $result->get_theme_json()->get_raw_data());
        }
        return array('config', $result->get_data());
    }

    /** `WP_Theme_JSON_Resolver::get_user_data_from_wp_global_styles()`'s query and post creation, as WordPress makes them. */
    function global_styles_post($stylesheet, $create_post, $post_status_filter)
    {
        $user_cpt         = array();
        $post_type_filter = 'wp_global_styles';
        $args             = array(
            'posts_per_page'         => 1,
            'orderby'                => 'date',
            'order'                  => 'desc',
            'post_type'              => $post_type_filter,
            'post_status'            => $post_status_filter,
            'ignore_sticky_posts'    => true,
            'no_found_rows'          => true,
            'update_post_meta_cache' => false,
            'update_post_term_cache' => false,
            'tax_query'              => array(
                array(
                    'taxonomy' => 'wp_theme',
                    'field'    => 'name',
                    'terms'    => $stylesheet,
                ),
            ),
        );
        $global_style_query = new \WP_Query();
        $recent_posts       = $global_style_query->query($args);
        if (count($recent_posts) === 1 && $recent_posts[0] instanceof \WP_Post) {
            $user_cpt = get_object_vars($recent_posts[0]);
        } elseif ($create_post) {
            $cpt_post_id = wp_insert_post(
                array(
                    'post_content' => '{"version": ' . \WP_Theme_JSON::LATEST_SCHEMA . ', "isGlobalStylesUserThemeJSON": true }',
                    'post_status'  => 'publish',
                    'post_title'   => 'Custom Styles',
                    'post_type'    => $post_type_filter,
                    'post_name'    => sprintf('wp-global-styles-%s', urlencode($stylesheet)),
                    'tax_input'    => array(
                        'wp_theme' => array($stylesheet),
                    ),
                ),
                true
            );
            if (!is_wp_error($cpt_post_id)) {
                $post = get_post($cpt_post_id);
                if ($post instanceof \WP_Post) {
                    $user_cpt = get_object_vars($post);
                }
            }
        }
        return $user_cpt;
    }
}

namespace
{
    use SpeedySpec\WpThemeJson\Native as N;

    /**
     * Holds a theme.json document; every method is answered by the Rust engine.
     */
    #[AllowDynamicProperties]
    class WP_Theme_JSON
    {
        /*CONSTANTS*/

        private $speedyspec_handle = -1;

        /**
         * A `WP_Theme_JSON` over a document the engine already holds (the resolver's results).
         */
        public static function speedyspec_from_handle( $handle )
        {
            $theme_json                    = ( new ReflectionClass( static::class ) )->newInstanceWithoutConstructor();
            $theme_json->speedyspec_handle = $handle;
            return $theme_json;
        }

        /**
         * The engine's handle for this document (for the resolver class).
         */
        public function speedyspec_handle()
        {
            return $this->speedyspec_handle;
        }

        public function __construct( $theme_json = array( 'version' => self::LATEST_SCHEMA ), $origin = 'theme' )
        {
            $this->speedyspec_handle = N\construct( $theme_json, $origin );
        }

        public function __clone()
        {
            $this->speedyspec_handle = N\duplicate( $this->speedyspec_handle );
        }

        public function __destruct()
        {
            N\release( $this->speedyspec_handle );
        }

        /**
         * Serializes the document itself, under WordPress's own property name, never the per-request handle.
         */
        public function __serialize(): array
        {
            return array( "\0*\0theme_json" => $this->get_raw_data() );
        }

        /**
         * Restores a serialized document, whether this class or WordPress's own wrote it.
         */
        public function __unserialize( array $data ): void
        {
            $raw = $data["\0*\0theme_json"] ?? $data['theme_json'] ?? array( 'version' => self::LATEST_SCHEMA );
            $this->speedyspec_handle = N\restore( $raw );
        }

        public function merge( $incoming )
        {
            N\merge( $this->speedyspec_handle, $incoming->get_raw_data() );
        }

        public function get_raw_data()
        {
            return N\get_raw_data( $this->speedyspec_handle );
        }

        public function get_settings()
        {
            return N\get_settings( $this->speedyspec_handle );
        }

        public function get_data()
        {
            return N\get_data( $this->speedyspec_handle );
        }

        public function get_custom_templates()
        {
            return N\get_custom_templates( $this->speedyspec_handle );
        }

        public function get_template_parts()
        {
            return N\get_template_parts( $this->speedyspec_handle );
        }

        public function get_patterns()
        {
            return N\get_patterns( $this->speedyspec_handle );
        }

        public function set_spacing_sizes()
        {
            N\set_spacing_sizes( $this->speedyspec_handle );
        }

        public function get_stylesheet( $types = array( 'variables', 'styles', 'presets' ), $origins = null, $options = array() )
        {
            return N\get_stylesheet( $this->speedyspec_handle, $types, $origins, $options );
        }

        public function get_styles_for_block( $block_metadata )
        {
            return N\get_styles_for_block( $this->speedyspec_handle, $block_metadata );
        }

        public function get_styles_block_nodes()
        {
            return N\get_styles_block_nodes( $this->speedyspec_handle );
        }

        public function get_root_layout_rules( $selector, $block_metadata, $options = array() )
        {
            return N\get_root_layout_rules( $this->speedyspec_handle, $block_metadata, $options );
        }

        public function get_custom_css()
        {
            return N\get_custom_css( $this->speedyspec_handle );
        }

        public static function scope_selector( $scope, $selector )
        {
            if ( ! $scope || ! $selector ) {
                return $selector;
            }
            return N\scope_selector( $scope, $selector );
        }

        public static function process_blocks_custom_css( $css, $selector )
        {
            return N\process_blocks_custom_css( $css, $selector );
        }

        public static function remove_insecure_properties( $theme_json, $origin = 'theme' )
        {
            return N\remove_insecure_properties( $theme_json, $origin );
        }

        public function get_svg_filters( $origins )
        {
            return N\get_svg_filters( $this->speedyspec_handle, $origins );
        }

        /**
         * Replaces the variables in the document's styles, in place, and returns the same object.
         */
        public static function resolve_variables( $theme_json )
        {
            N\resolve_variables( $theme_json->speedyspec_handle );
            return $theme_json;
        }

        public static function get_element_class_name( $element )
        {
            return N\get_element_class_name( $element );
        }

        public static function get_from_editor_settings( $settings )
        {
            return N\get_from_editor_settings( $settings );
        }

        public static function get_viewport_media_queries( $viewport_settings = null, $options = array() )
        {
            return N\get_viewport_media_queries( $viewport_settings, ! empty( $options['include_desktop'] ) );
        }
    }

    /**
     * Migrates theme.json documents to the latest schema, in the Rust engine.
     */
    class WP_Theme_JSON_Schema
    {
        /*SCHEMA_CONSTANTS*/

        public static function migrate( $theme_json, $origin = 'theme' )
        {
            return N\schema_migrate( $theme_json, $origin );
        }
    }

    /**
     * What the `wp_theme_json_data_*` filters receive: one layer, which a callback may update.
     */
    #[AllowDynamicProperties]
    class WP_Theme_JSON_Data
    {
        private $theme_json = null;

        private $origin = '';

        public function __construct( $data = array( 'version' => WP_Theme_JSON::LATEST_SCHEMA ), $origin = 'theme' )
        {
            $this->origin     = $origin;
            $this->theme_json = new WP_Theme_JSON( $data, $this->origin );
        }

        public function update_with( $new_data )
        {
            $this->theme_json->merge( new WP_Theme_JSON( $new_data, $this->origin ) );
            return $this;
        }

        public function get_data()
        {
            return $this->theme_json->get_raw_data();
        }

        public function get_theme_json()
        {
            return $this->theme_json;
        }
    }

    /**
     * Resolves the theme.json layers (core, blocks, theme, user) in the Rust engine.
     */
    #[AllowDynamicProperties]
    class WP_Theme_JSON_Resolver
    {
        private static function wrap( $handle )
        {
            return WP_Theme_JSON::speedyspec_from_handle( $handle );
        }

        public static function get_fields_to_translate()
        {
            return N\resolver_fields_to_translate();
        }

        public static function get_core_data()
        {
            return self::wrap( N\resolver_core_data() );
        }

        public static function get_theme_data( $deprecated = array(), $options = array() )
        {
            $options = wp_parse_args( $options, array( 'with_supports' => true ) );
            return self::wrap( N\resolver_theme_data( $deprecated, (bool) $options['with_supports'] ) );
        }

        public static function get_block_data()
        {
            return self::wrap( N\resolver_block_data() );
        }

        public static function get_user_data_from_wp_global_styles( $theme, $create_post = false, $post_status_filter = array( 'publish' ) )
        {
            if ( ! $theme instanceof WP_Theme ) {
                $theme = wp_get_theme();
            }
            return SpeedySpec\WpThemeJson\Support\global_styles_post( $theme->get_stylesheet(), $create_post, $post_status_filter );
        }

        public static function get_user_data()
        {
            return self::wrap( N\resolver_user_data() );
        }

        public static function get_merged_data( $origin = 'custom' )
        {
            return self::wrap( N\resolver_merged_data( $origin ) );
        }

        public static function get_user_global_styles_post_id()
        {
            return N\resolver_user_post_id();
        }

        public static function theme_has_support()
        {
            return N\resolver_theme_has_support();
        }

        public static function clean_cached_data()
        {
            N\resolver_clean_cached_data();
        }

        public static function get_style_variations( $scope = 'theme' )
        {
            return N\resolver_style_variations( $scope );
        }

        public static function get_resolved_theme_uris( $theme_json )
        {
            if ( ! $theme_json instanceof WP_Theme_JSON ) {
                return array();
            }
            return N\resolver_resolved_theme_uris( $theme_json->speedyspec_handle() );
        }

        public static function resolve_theme_file_uris( $theme_json )
        {
            $resolved = self::get_resolved_theme_uris( $theme_json );
            if ( empty( $resolved ) ) {
                return $theme_json;
            }
            return self::wrap( N\resolver_resolve_theme_file_uris( $theme_json->speedyspec_handle() ) );
        }
    }

    function wp_get_global_settings( $path = array(), $context = array() )
    {
        return N\global_settings( $path, $context );
    }

    function wp_get_global_styles( $path = array(), $context = array() )
    {
        return N\global_styles( $path, $context );
    }

    function wp_get_global_stylesheet( $types = array() )
    {
        return N\global_stylesheet( $types );
    }

    function wp_get_block_name_from_theme_json_path( $path )
    {
        return N\block_name_from_theme_json_path( $path );
    }

    function wp_clean_theme_json_cache()
    {
        N\clean_theme_json_cache();
    }

    function wp_get_theme_directory_pattern_slugs()
    {
        return N\theme_directory_pattern_slugs();
    }

    function wp_get_theme_data_custom_templates()
    {
        return N\theme_data_custom_templates();
    }

    function wp_get_theme_data_template_parts()
    {
        return N\theme_data_template_parts();
    }

    /**
     * WordPress's own: whether the active theme has a `theme.json` (a filtered file check, kept per stylesheet).
     */
    function wp_theme_has_theme_json()
    {
        static $theme_has_support = array();
        $stylesheet = get_stylesheet();
        if ( isset( $theme_has_support[ $stylesheet ] ) && ! wp_is_development_mode( 'theme' ) ) {
            return $theme_has_support[ $stylesheet ];
        }
        $stylesheet_directory = get_stylesheet_directory();
        $template_directory   = get_template_directory();
        if ( $stylesheet_directory !== $template_directory && file_exists( $stylesheet_directory . '/theme.json' ) ) {
            $path = $stylesheet_directory . '/theme.json';
        } else {
            $path = $template_directory . '/theme.json';
        }
        $path = apply_filters( 'theme_file_path', $path, 'theme.json' );
        $theme_has_support[ $stylesheet ] = file_exists( $path );
        return $theme_has_support[ $stylesheet ];
    }

    /**
     * WordPress's own: each block's global styles, added to the style queue (cached in a transient).
     */
    function wp_add_global_styles_for_blocks()
    {
        global $wp_styles;
        $tree           = WP_Theme_JSON_Resolver::get_merged_data();
        $tree           = WP_Theme_JSON_Resolver::resolve_theme_file_uris( $tree );
        $block_nodes    = $tree->get_styles_block_nodes();
        $can_use_cached = ! wp_is_development_mode( 'theme' );
        $update_cache   = false;
        if ( $can_use_cached ) {
            $cache_hash = md5( wp_json_encode( $tree->get_raw_data() ) );
            $cache_key  = 'wp_styles_for_blocks';
            $cached     = get_transient( $cache_key );
            if ( ! is_array( $cached ) || $cached['hash'] !== $cache_hash ) {
                $cached       = array(
                    'hash'   => $cache_hash,
                    'blocks' => array(),
                );
                $update_cache = true;
            }
        }
        foreach ( $block_nodes as $metadata ) {
            if ( $can_use_cached ) {
                $cache_node_key = md5( wp_json_encode( $metadata ) );
                if ( isset( $cached['blocks'][ $cache_node_key ] ) ) {
                    $block_css = $cached['blocks'][ $cache_node_key ];
                } else {
                    $block_css                           = $tree->get_styles_for_block( $metadata );
                    $cached['blocks'][ $cache_node_key ] = $block_css;
                    $update_cache                        = true;
                }
            } else {
                $block_css = $tree->get_styles_for_block( $metadata );
            }
            if ( ! wp_should_load_block_assets_on_demand() ) {
                wp_add_inline_style( 'global-styles', $block_css );
                continue;
            }
            $stylesheet_handle = 'global-styles';
            if ( isset( $metadata['name'] ) ) {
                if ( str_starts_with( $metadata['name'], 'core/' ) ) {
                    $block_name   = str_replace( 'core/', '', $metadata['name'] );
                    $block_handle = 'wp-block-' . $block_name;
                    if ( in_array( $block_handle, $wp_styles->queue, true ) ) {
                        wp_add_inline_style( $stylesheet_handle, $block_css );
                    }
                } else {
                    wp_add_inline_style( $stylesheet_handle, $block_css );
                }
            }
            if ( ! isset( $metadata['name'] ) && ! empty( $metadata['path'] ) ) {
                $block_name = wp_get_block_name_from_theme_json_path( $metadata['path'] );
                if ( $block_name ) {
                    if ( str_starts_with( $block_name, 'core/' ) ) {
                        $block_name   = str_replace( 'core/', '', $block_name );
                        $block_handle = 'wp-block-' . $block_name;
                        if ( in_array( $block_handle, $wp_styles->queue, true ) ) {
                            wp_add_inline_style( $stylesheet_handle, $block_css );
                        }
                    } else {
                        wp_add_inline_style( $stylesheet_handle, $block_css );
                    }
                }
            }
        }
        if ( $update_cache ) {
            set_transient( $cache_key, $cached );
        }
    }

    if ( ! function_exists( 'wp_get_block_css_selector' ) ) {
        /**
         * Returns a block's CSS selector for a feature or subfeature, answered by the Rust engine.
         */
        function wp_get_block_css_selector( $block_type, $target = 'root', $fallback = false )
        {
            return N\block_css_selector(
                $block_type->name ?? '',
                $block_type->selectors ?? array(),
                $block_type->supports ?? null,
                $target,
                $fallback
            );
        }
    }
}
