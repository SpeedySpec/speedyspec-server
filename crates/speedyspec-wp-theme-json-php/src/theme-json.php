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
