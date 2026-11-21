// Evaluated without an opening tag. No strict_types: WordPress's own l10n files are not strict, so arguments coerce as
// they do there.
//
// The translation controller and file classes are shells over the Rust controller (speedyspec-wp-i18n);
// WP_Translations is WordPress 7.1.5's own class (GPL-2.0-or-later) with one added line. The Legacy namespace holds
// WordPress 7.1.5's translate(), translate_with_gettext_context(), __(), _x(), _n() and _nx(): the native functions
// of those names serve the common case and hand every other call to these.

namespace SpeedySpec\WpI18n\Legacy
{
    function translate( $text, $domain = 'default' ) {
        $translations = get_translations_for_domain( $domain );
        $translation  = $translations->translate( $text );
        $translation = apply_filters( 'gettext', $translation, $text, $domain );
        $translation = apply_filters( "gettext_{$domain}", $translation, $text, $domain );
        return $translation;
    }

    function translate_with_gettext_context( $text, $context, $domain = 'default' ) {
        $translations = get_translations_for_domain( $domain );
        $translation  = $translations->translate( $text, $context );
        $translation = apply_filters( 'gettext_with_context', $translation, $text, $context, $domain );
        $translation = apply_filters( "gettext_with_context_{$domain}", $translation, $text, $context, $domain );
        return $translation;
    }

    function __( $text, $domain = 'default' ) {
        return translate( $text, $domain );
    }

    // The two filters of each function, for a translation the native function found: the same calls, in the same
    // order, with the same arguments as WordPress's. Each takes its function's own parameters; the translation is
    // handed over by take_translation().

    function filter_gettext( $text, $domain = 'default' ) {
        $translation = \SpeedySpec\WpI18n\Native\take_translation();
        $translation = apply_filters( 'gettext', $translation, $text, $domain );
        return apply_filters( "gettext_{$domain}", $translation, $text, $domain );
    }

    function filter_gettext_with_context( $text, $context, $domain = 'default' ) {
        $translation = \SpeedySpec\WpI18n\Native\take_translation();
        $translation = apply_filters( 'gettext_with_context', $translation, $text, $context, $domain );
        return apply_filters( "gettext_with_context_{$domain}", $translation, $text, $context, $domain );
    }

    function filter_ngettext( $single, $plural, $number, $domain = 'default' ) {
        $translation = \SpeedySpec\WpI18n\Native\take_translation();
        $translation = apply_filters( 'ngettext', $translation, $single, $plural, $number, $domain );
        return apply_filters( "ngettext_{$domain}", $translation, $single, $plural, $number, $domain );
    }

    function filter_ngettext_with_context( $single, $plural, $number, $context, $domain = 'default' ) {
        $translation = \SpeedySpec\WpI18n\Native\take_translation();
        $translation = apply_filters( 'ngettext_with_context', $translation, $single, $plural, $number, $context, $domain );
        return apply_filters( "ngettext_with_context_{$domain}", $translation, $single, $plural, $number, $context, $domain );
    }

    function _x( $text, $context, $domain = 'default' ) {
        return translate_with_gettext_context( $text, $context, $domain );
    }

    function _n( $single, $plural, $number, $domain = 'default' ) {
        $translations = get_translations_for_domain( $domain );
        $translation  = $translations->translate_plural( $single, $plural, $number );
        $translation = apply_filters( 'ngettext', $translation, $single, $plural, $number, $domain );
        $translation = apply_filters( "ngettext_{$domain}", $translation, $single, $plural, $number, $domain );
        return $translation;
    }

    function _nx( $single, $plural, $number, $context, $domain = 'default' ) {
        $translations = get_translations_for_domain( $domain );
        $translation  = $translations->translate_plural( $single, $plural, $number, $context );
        $translation = apply_filters( 'ngettext_with_context', $translation, $single, $plural, $number, $context, $domain );
        $translation = apply_filters( "ngettext_with_context_{$domain}", $translation, $single, $plural, $number, $context, $domain );
        return $translation;
    }
}

namespace SpeedySpec\WpI18n
{
    /**
     * Runs a `.l10n.php` file the Rust reader cannot parse, as WP_Translation_File_PHP::parse_file() does.
     *
     * @return array|string
     *   `[headers, entries]`, or WordPress's error text.
     */
    function include_translation_file( string $file ) {
        $result = include $file;
        if ( ! $result || ! is_array( $result ) ) {
            return 'Invalid data';
        }
        $entries = array();
        if ( isset( $result['messages'] ) && is_array( $result['messages'] ) ) {
            foreach ( $result['messages'] as $original => $translation ) {
                $entries[ (string) $original ] = $translation;
            }
            unset( $result['messages'] );
        }
        return array( array_change_key_case( $result ), $entries );
    }
}

namespace
{
    final class WP_Translation_Controller {
        /** @var int|null The Rust controller this object stands for; 0 is the request's own. */
        private $speedyspec_id;

        private static $instance = null;

        /** The Rust controller's id, made on first use (WordPress's class has no constructor). */
        private function speedyspec_id(): int {
            if ( null === $this->speedyspec_id ) {
                $this->speedyspec_id = \SpeedySpec\WpI18n\Native\controller_new();
            }
            return $this->speedyspec_id;
        }

        public static function get_instance(): WP_Translation_Controller {
            if ( null === self::$instance ) {
                $instance = ( new ReflectionClass( self::class ) )->newInstanceWithoutConstructor();
                $instance->speedyspec_id = \SpeedySpec\WpI18n\Native\controller_main();
                self::$instance = $instance;
            }
            return self::$instance;
        }

        public function get_locale(): string {
            return \SpeedySpec\WpI18n\Native\controller_get_locale( $this->speedyspec_id() );
        }

        public function set_locale( string $locale ) {
            \SpeedySpec\WpI18n\Native\controller_set_locale( $this->speedyspec_id(), $locale );
        }

        public function load_file( string $translation_file, string $textdomain = 'default', ?string $locale = null ): bool {
            return \SpeedySpec\WpI18n\Native\controller_load_file( $this->speedyspec_id(), $translation_file, $textdomain, $locale );
        }

        public function unload_file( $file, string $textdomain = 'default', ?string $locale = null ): bool {
            if ( $file instanceof WP_Translation_File ) {
                // A file object made outside the controller is never one of its own, so nothing matches.
                return null !== $locale;
            }
            return \SpeedySpec\WpI18n\Native\controller_unload_file( $this->speedyspec_id(), is_string( $file ) ? $file : null, $textdomain, $locale );
        }

        public function unload_textdomain( string $textdomain = 'default', ?string $locale = null ): bool {
            return \SpeedySpec\WpI18n\Native\controller_unload_textdomain( $this->speedyspec_id(), $textdomain, $locale );
        }

        public function is_textdomain_loaded( string $textdomain = 'default', ?string $locale = null ): bool {
            return \SpeedySpec\WpI18n\Native\controller_is_textdomain_loaded( $this->speedyspec_id(), $textdomain, $locale );
        }

        public function translate( string $text, string $context = '', string $textdomain = 'default', ?string $locale = null ) {
            return \SpeedySpec\WpI18n\Native\controller_translate( $this->speedyspec_id(), $text, $context, $textdomain, $locale );
        }

        public function translate_plural( array $plurals, int $number, string $context = '', string $textdomain = 'default', ?string $locale = null ) {
            return \SpeedySpec\WpI18n\Native\controller_translate_plural( $this->speedyspec_id(), array_values( array_map( 'strval', $plurals ) ), $number, $context, $textdomain, $locale );
        }

        public function get_headers( string $textdomain = 'default' ): array {
            return \SpeedySpec\WpI18n\Native\controller_get_headers( $this->speedyspec_id(), $textdomain );
        }

        public function get_entries( string $textdomain = 'default' ): array {
            return \SpeedySpec\WpI18n\Native\controller_get_entries( $this->speedyspec_id(), $textdomain );
        }

        public function has_translation( string $singular, string $textdomain = 'default', ?string $locale = null ): bool {
            return \SpeedySpec\WpI18n\Native\controller_has_translation( $this->speedyspec_id(), $singular, $textdomain, $locale );
        }

        // WordPress's protected helpers, kept for the class's shape; the class is final, so nothing outside calls
        // them, and the Rust controller does their work.

        protected function normalize_header( string $header ): string {
            $parts = explode( '-', $header );
            $parts = array_map( 'ucfirst', $parts );
            return implode( '-', $parts );
        }

        protected function locate_translation( string $singular, string $textdomain = 'default', ?string $locale = null ) {
            $translation = $this->translate( $singular, '', $textdomain, $locale );
            return false === $translation ? false : array( 'entries' => array( $translation ), 'source' => null );
        }

        protected function get_files( string $textdomain = 'default', ?string $locale = null ): array {
            return array();
        }
    }

    abstract class WP_Translation_File {
        /** @var int|null The Rust file object; null for a file being built by transform(). */
        protected $speedyspec_id;

        protected $headers = array();
        protected $parsed = false;
        protected $error;
        protected $file = '';
        protected $entries = array();
        protected $plural_forms = null;

        protected function __construct( string $file ) {
            $this->file = $file;
        }

        public static function create( string $file, ?string $filetype = null ) {
            $id = \SpeedySpec\WpI18n\Native\file_open( $file, $filetype );
            if ( false === $id ) {
                return false;
            }
            $instance = 'php' === \SpeedySpec\WpI18n\Native\file_type( $file, $filetype ) ? new WP_Translation_File_PHP( $file ) : new WP_Translation_File_MO( $file );
            $instance->speedyspec_id = $id;
            return $instance;
        }

        public static function transform( string $file, string $filetype ) {
            $source = self::create( $file );
            if ( false === $source ) {
                return false;
            }
            switch ( $filetype ) {
                case 'mo':
                    $destination = new WP_Translation_File_MO( '' );
                    break;
                case 'php':
                    $destination = new WP_Translation_File_PHP( '' );
                    break;
                default:
                    return false;
            }
            $success = $destination->import( $source );
            if ( ! $success ) {
                return false;
            }
            return $destination->export();
        }

        public function headers(): array {
            if ( ! $this->parsed ) {
                $this->parse_file();
            }
            return $this->headers;
        }

        public function entries(): array {
            if ( ! $this->parsed ) {
                $this->parse_file();
            }
            return $this->entries;
        }

        public function error() {
            return $this->error;
        }

        public function get_file(): string {
            return $this->file;
        }

        public function translate( string $text ) {
            if ( ! $this->parsed ) {
                $this->parse_file();
            }
            return $this->entries[ $text ] ?? false;
        }

        public function get_plural_form( int $number ): int {
            if ( ! $this->parsed ) {
                $this->parse_file();
            }
            if ( null !== $this->speedyspec_id ) {
                return \SpeedySpec\WpI18n\Native\file_plural_form( $this->speedyspec_id, $number );
            }
            if ( null === $this->plural_forms && isset( $this->headers['plural-forms'] ) ) {
                $expression         = $this->get_plural_expression_from_header( $this->headers['plural-forms'] );
                $this->plural_forms = $this->make_plural_form_function( $expression );
            }
            if ( is_callable( $this->plural_forms ) ) {
                return call_user_func( $this->plural_forms, $number );
            }
            return ( 1 === $number ? 0 : 1 );
        }

        protected function get_plural_expression_from_header( string $header ): string {
            if ( preg_match( '/^\s*nplurals\s*=\s*(\d+)\s*;\s+plural\s*=\s*(.+)$/', $header, $matches ) ) {
                return trim( $matches[2] );
            }
            return 'n != 1';
        }

        protected function make_plural_form_function( string $expression ): callable {
            try {
                $handler = new Plural_Forms( rtrim( $expression, ';' ) );
                return array( $handler, 'get' );
            } catch ( Exception $e ) {
                return $this->make_plural_form_function( 'n != 1' );
            }
        }

        protected function import( WP_Translation_File $source ): bool {
            if ( null !== $source->error() ) {
                return false;
            }
            $this->headers = $source->headers();
            $this->entries = $source->entries();
            $this->error   = $source->error();
            $this->parsed  = true;
            return null === $this->error;
        }

        abstract protected function parse_file();

        abstract public function export();
    }

    class WP_Translation_File_MO extends WP_Translation_File {
        protected $uint32 = false;

        const MAGIC_MARKER = 0x950412de;

        protected function detect_endian_and_validate_file( string $header ) {
            $big = unpack( 'N', $header );
            if ( false === $big ) {
                return false;
            }
            $big = reset( $big );
            if ( false === $big ) {
                return false;
            }
            $little = unpack( 'V', $header );
            if ( false === $little ) {
                return false;
            }
            $little = reset( $little );
            if ( false === $little ) {
                return false;
            }
            if ( (int) self::MAGIC_MARKER === $big ) {
                return 'N';
            }
            if ( (int) self::MAGIC_MARKER === $little ) {
                return 'V';
            }
            $this->error = 'Magic marker does not exist';
            return false;
        }

        protected function parse_file(): bool {
            // Headers, entries and error from the Rust file, as WordPress parses on first use.
            $this->parsed = true;
            if ( null !== $this->speedyspec_id ) {
                list( $this->headers, $this->entries, $this->error ) = \SpeedySpec\WpI18n\Native\file_parse( $this->speedyspec_id );
            }
            return null === $this->error;
        }

        public function export(): string {
            if ( ! $this->parsed ) {
                $this->parse_file();
            }
            // WordPress 7.1.5's WP_Translation_File_MO::export().
            $headers_string = '';
            foreach ( $this->headers as $header => $value ) {
                $headers_string .= "{$header}: $value\n";
            }
            $entries     = array_merge( array( '' => $headers_string ), $this->entries );
            $entry_count = count( $entries );
            if ( false === $this->uint32 ) {
                $this->uint32 = 'V';
            }
            $bytes_for_entries = $entry_count * 4 * 2;
            $originals_addr    = 28;
            $translations_addr = $originals_addr + $bytes_for_entries;
            $hash_addr         = $translations_addr + $bytes_for_entries;
            $entry_offsets     = $hash_addr;
            $file_header = pack( $this->uint32 . '*', (int) self::MAGIC_MARKER, 0, $entry_count, $originals_addr, $translations_addr, 0, $hash_addr );
            $o_entries = '';
            $t_entries = '';
            $o_addr    = '';
            $t_addr    = '';
            foreach ( array_keys( $entries ) as $original ) {
                $o_addr        .= pack( $this->uint32 . '*', strlen( $original ), $entry_offsets );
                $entry_offsets += strlen( $original ) + 1;
                $o_entries     .= $original . "\0";
            }
            foreach ( $entries as $translations ) {
                $t_addr        .= pack( $this->uint32 . '*', strlen( $translations ), $entry_offsets );
                $entry_offsets += strlen( $translations ) + 1;
                $t_entries     .= $translations . "\0";
            }
            return $file_header . $o_addr . $t_addr . $o_entries . $t_entries;
        }
    }

    class WP_Translation_File_PHP extends WP_Translation_File {
        protected function parse_file() {
            // Headers, entries and error from the Rust file, as WordPress parses on first use.
            $this->parsed = true;
            if ( null !== $this->speedyspec_id ) {
                list( $this->headers, $this->entries, $this->error ) = \SpeedySpec\WpI18n\Native\file_parse( $this->speedyspec_id );
            }
        }

        public function export(): string {
            if ( ! $this->parsed ) {
                $this->parse_file();
            }
            // WordPress 7.1.5's WP_Translation_File_PHP::export().
            $data = array_merge( $this->headers, array( 'messages' => $this->entries ) );
            return '<?php' . PHP_EOL . 'return ' . $this->var_export( $data ) . ';' . PHP_EOL;
        }

        private function var_export( $value ): string {
            if ( ! is_array( $value ) ) {
                return var_export( $value, true );
            }
            $entries = array();
            $is_list = array_is_list( $value );
            foreach ( $value as $key => $val ) {
                $entries[] = $is_list ? $this->var_export( $val ) : var_export( $key, true ) . '=>' . $this->var_export( $val );
            }
            return '[' . implode( ',', $entries ) . ']';
        }
    }

    /**
     * I18N: WP_Translations class.
     *
     * @package WordPress
     * @subpackage I18N
     * @since 6.5.0
     */

    /**
     * Class WP_Translations.
     *
     * @since 6.5.0
     *
     * @property-read array<string, string> $headers
     * @property-read array<string, string[]> $entries
     */
    class WP_Translations {
    	/**
    	 * Text domain.
    	 *
    	 * @since 6.5.0
    	 * @var string
    	 */
    	protected $textdomain = 'default';

    	/**
    	 * Translation controller instance.
    	 *
    	 * @since 6.5.0
    	 * @var WP_Translation_Controller
    	 */
    	protected $controller;

    	/**
    	 * Constructor.
    	 *
    	 * @since 6.5.0
    	 *
    	 * @param WP_Translation_Controller $controller I18N controller.
    	 * @param string                    $textdomain Optional. Text domain. Default 'default'.
    	 */
    	public function __construct( WP_Translation_Controller $controller, string $textdomain = 'default' ) {
    		$this->controller = $controller;
    		$this->textdomain = $textdomain;
    		// SpeedySpec: lets the native __() family recognize this object in $l10n.
    		\SpeedySpec\WpI18n\Native\register_translations( $this, $textdomain, ( function () { return $this->speedyspec_id(); } )->call( $controller ) );
    	}

    	/**
    	 * Magic getter for backward compatibility.
    	 *
    	 * @since 6.5.0
    	 *
    	 * @param string $name Property name.
    	 * @return mixed
    	 */
    	public function __get( string $name ) {
    		if ( 'entries' === $name ) {
    			$entries = $this->controller->get_entries( $this->textdomain );

    			$result = array();

    			foreach ( $entries as $original => $translations ) {
    				$result[] = $this->make_entry( $original, $translations );
    			}

    			return $result;
    		}

    		if ( 'headers' === $name ) {
    			return $this->controller->get_headers( $this->textdomain );
    		}

    		return null;
    	}

    	/**
    	 * Builds a Translation_Entry from original string and translation strings.
    	 *
    	 * @see MO::make_entry()
    	 *
    	 * @since 6.5.0
    	 *
    	 * @param string $original     Original string to translate from MO file. Might contain
    	 *                             0x04 as context separator or 0x00 as singular/plural separator.
    	 * @param string $translations Translation strings from MO file.
    	 * @return Translation_Entry Entry instance.
    	 */
    	private function make_entry( $original, $translations ): Translation_Entry {
    		$entry = new Translation_Entry();

    		// Look for context, separated by \4.
    		$parts = explode( "\4", $original );
    		if ( isset( $parts[1] ) ) {
    			$original       = $parts[1];
    			$entry->context = $parts[0];
    		}

    		$entry->singular     = $original;
    		$entry->translations = explode( "\0", $translations );
    		$entry->is_plural    = count( $entry->translations ) > 1;

    		return $entry;
    	}

    	/**
    	 * Translates a plural string.
    	 *
    	 * @since 6.5.0
    	 *
    	 * @param string|null $singular Singular string.
    	 * @param string|null $plural   Plural string.
    	 * @param int|float   $count    Count. Should be an integer, but some plugins pass floats.
    	 * @param string|null $context  Context.
    	 * @return string|null Translation if it exists, or the unchanged singular string.
    	 */
    	public function translate_plural( $singular, $plural, $count = 1, $context = '' ) {
    		if ( null === $singular || null === $plural ) {
    			return $singular;
    		}

    		$translation = $this->controller->translate_plural( array( $singular, $plural ), (int) $count, (string) $context, $this->textdomain );
    		if ( false !== $translation ) {
    			return $translation;
    		}

    		// Fall back to the original with English grammar rules.
    		return ( 1 === (int) $count ? $singular : $plural );
    	}

    	/**
    	 * Translates a singular string.
    	 *
    	 * @since 6.5.0
    	 *
    	 * @param string|null $singular Singular string.
    	 * @param string|null $context  Context.
    	 * @return string|null Translation if it exists, or the unchanged singular string
    	 */
    	public function translate( $singular, $context = '' ) {
    		if ( null === $singular ) {
    			return null;
    		}

    		$translation = $this->controller->translate( $singular, (string) $context, $this->textdomain );
    		if ( false !== $translation ) {
    			return $translation;
    		}

    		// Fall back to the original.
    		return $singular;
    	}
    }
}
