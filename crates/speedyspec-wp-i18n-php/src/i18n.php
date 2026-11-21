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
     * WP_Locale::init()'s translations, in WordPress's order: natively when $l10n['default'] is the port's own and
     * nothing is hooked to gettext or gettext_with_context, else through __() and _x() as WordPress makes them.
     * The text direction is looked up last, only when $GLOBALS['text_direction'] is not set by then.
     *
     * @return array<int, mixed>
     *     64 translations, then the text direction's when it was looked up.
     */
    function locale_strings(): array {
        $t = Native\locale_strings( ! isset( $GLOBALS['text_direction'] ) );
        if ( null !== $t ) {
            return $t;
        }
        $t = array(
            __( 'Sunday' ),
            __( 'Monday' ),
            __( 'Tuesday' ),
            __( 'Wednesday' ),
            __( 'Thursday' ),
            __( 'Friday' ),
            __( 'Saturday' ),
            _x( 'S', 'Sunday initial' ),
            _x( 'M', 'Monday initial' ),
            _x( 'T', 'Tuesday initial' ),
            _x( 'W', 'Wednesday initial' ),
            _x( 'T', 'Thursday initial' ),
            _x( 'F', 'Friday initial' ),
            _x( 'S', 'Saturday initial' ),
            __( 'Sun' ),
            __( 'Mon' ),
            __( 'Tue' ),
            __( 'Wed' ),
            __( 'Thu' ),
            __( 'Fri' ),
            __( 'Sat' ),
            __( 'January' ),
            __( 'February' ),
            __( 'March' ),
            __( 'April' ),
            __( 'May' ),
            __( 'June' ),
            __( 'July' ),
            __( 'August' ),
            __( 'September' ),
            __( 'October' ),
            __( 'November' ),
            __( 'December' ),
            _x( 'January', 'genitive' ),
            _x( 'February', 'genitive' ),
            _x( 'March', 'genitive' ),
            _x( 'April', 'genitive' ),
            _x( 'May', 'genitive' ),
            _x( 'June', 'genitive' ),
            _x( 'July', 'genitive' ),
            _x( 'August', 'genitive' ),
            _x( 'September', 'genitive' ),
            _x( 'October', 'genitive' ),
            _x( 'November', 'genitive' ),
            _x( 'December', 'genitive' ),
            _x( 'Jan', 'January abbreviation' ),
            _x( 'Feb', 'February abbreviation' ),
            _x( 'Mar', 'March abbreviation' ),
            _x( 'Apr', 'April abbreviation' ),
            _x( 'May', 'May abbreviation' ),
            _x( 'Jun', 'June abbreviation' ),
            _x( 'Jul', 'July abbreviation' ),
            _x( 'Aug', 'August abbreviation' ),
            _x( 'Sep', 'September abbreviation' ),
            _x( 'Oct', 'October abbreviation' ),
            _x( 'Nov', 'November abbreviation' ),
            _x( 'Dec', 'December abbreviation' ),
            __( 'am' ),
            __( 'pm' ),
            __( 'AM' ),
            __( 'PM' ),
            __( 'number_format_thousands_sep' ),
            __( 'number_format_decimal_point' ),
            __( ', ' ),
        );
        if ( ! isset( $GLOBALS['text_direction'] ) ) {
            $t[] = _x( 'ltr', 'text direction' );
        }
        return $t;
    }

    /**
     * WP_Locale_Switcher's private change_locale() before its `change_locale` action (which the native switcher
     * fires): the loaded domains' translations reloaded in `$locale` (its private load_translations()), a new
     * `$wp_locale`, the translation controller's locale, and PHPMailer's language. WordPress 7.1.5's code.
     *
     * @param string $locale
     *     The locale switched to.
     */
    function switcher_change_locale( $locale ): void {
        global $l10n, $wp_locale, $phpmailer;

        $domains = $l10n ? array_keys( $l10n ) : array();

        load_default_textdomain( $locale );

        foreach ( $domains as $domain ) {
            // The default text domain is handled by `load_default_textdomain()`.
            if ( 'default' === $domain ) {
                continue;
            }

            /*
             * Unload current text domain but allow them to be reloaded
             * after switching back or to another locale.
             */
            unload_textdomain( $domain, true );
            get_translations_for_domain( $domain );
        }

        $wp_locale = new \WP_Locale();

        \WP_Translation_Controller::get_instance()->set_locale( $locale );

        if ( $phpmailer instanceof \WP_PHPMailer ) {
            $phpmailer->setLanguage();
        }
    }

    /**
     * `$wp_textdomain_registry->get( $domain, $locale )`, for the native script translations.
     *
     * @param string $domain
     *     The text domain.
     * @param string $locale
     *     The locale.
     * @return string|false
     *     The languages directory for the domain, or false.
     */
    function registry_get( $domain, $locale ) {
        global $wp_textdomain_registry;
        return $wp_textdomain_registry->get( $domain, $locale );
    }

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
            if ( false === $this->uint32 ) {
                $this->uint32 = 'V';
            }
            // SpeedySpec: the object's own headers and entries, written natively when they are all strings.
            $exported = \SpeedySpec\WpI18n\Native\file_export_mo( $this->headers, $this->entries, $this->uint32 );
            if ( null !== $exported ) {
                return $exported;
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
            // SpeedySpec: the object's own headers and entries, written natively when they are all strings.
            $exported = \SpeedySpec\WpI18n\Native\file_export_php( $this->headers, $this->entries );
            if ( null !== $exported ) {
                return $exported;
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

    // WP_Locale is WordPress 7.1.5's own class (GPL-2.0-or-later) with init() taking its translations from
    // SpeedySpec\WpI18n\locale_strings().
    /**
     * Core class used to store translated data for a locale.
     *
     * @since 2.1.0
     * @since 4.6.0 Moved to its own file from wp-includes/locale.php.
     */
    #[AllowDynamicProperties]
    class WP_Locale {
    	/**
    	 * Stores the translated strings for the full weekday names.
    	 *
    	 * @since 2.1.0
    	 * @since 6.2.0 Initialized to an empty array.
    	 * @var string[]
    	 */
    	public $weekday = array();

    	/**
    	 * Stores the translated strings for the one character weekday names.
    	 *
    	 * There is a hack to make sure that Tuesday and Thursday, as well
    	 * as Sunday and Saturday, don't conflict. See init() method for more.
    	 *
    	 * @see WP_Locale::init() for how to handle the hack.
    	 *
    	 * @since 2.1.0
    	 * @since 6.2.0 Initialized to an empty array.
    	 * @var string[]
    	 */
    	public $weekday_initial = array();

    	/**
    	 * Stores the translated strings for the abbreviated weekday names.
    	 *
    	 * @since 2.1.0
    	 * @since 6.2.0 Initialized to an empty array.
    	 * @var string[]
    	 */
    	public $weekday_abbrev = array();

    	/**
    	 * Stores the translated strings for the full month names.
    	 *
    	 * @since 2.1.0
    	 * @since 6.2.0 Initialized to an empty array.
    	 * @var string[]
    	 */
    	public $month = array();

    	/**
    	 * Stores the translated strings for the month names in genitive case, if the locale specifies.
    	 *
    	 * @since 4.4.0
    	 * @since 6.2.0 Initialized to an empty array.
    	 * @var string[]
    	 */
    	public $month_genitive = array();

    	/**
    	 * Stores the translated strings for the abbreviated month names.
    	 *
    	 * @since 2.1.0
    	 * @since 6.2.0 Initialized to an empty array.
    	 * @var string[]
    	 */
    	public $month_abbrev = array();

    	/**
    	 * Stores the translated strings for 'am' and 'pm'.
    	 *
    	 * Also the capitalized versions.
    	 *
    	 * @since 2.1.0
    	 * @since 6.2.0 Initialized to an empty array.
    	 * @var string[]
    	 */
    	public $meridiem = array();

    	/**
    	 * The text direction of the locale language.
    	 *
    	 * Default is left to right 'ltr'.
    	 *
    	 * @since 2.1.0
    	 * @var string
    	 */
    	public $text_direction = 'ltr';

    	/**
    	 * The thousands separator and decimal point values used for localizing numbers.
    	 *
    	 * @since 2.3.0
    	 * @since 6.2.0 Initialized to an empty array.
    	 * @var array
    	 */
    	public $number_format = array();

    	/**
    	 * The separator string used for localizing list item separator.
    	 *
    	 * @since 6.0.0
    	 * @var string
    	 */
    	public $list_item_separator;

    	/**
    	 * The word count type of the locale language.
    	 *
    	 * Default is 'words'.
    	 *
    	 * @since 6.2.0
    	 * @var string
    	 */
    	public $word_count_type;

    	/**
    	 * Constructor which calls helper methods to set up object variables.
    	 *
    	 * @since 2.1.0
    	 */
    	public function __construct() {
    		$this->init();
    		$this->register_globals();
    	}

    	/**
    	 * Sets up the translated strings and object properties.
    	 *
    	 * The method creates the translatable strings for various
    	 * calendar elements. Which allows for specifying locale
    	 * specific calendar names and text direction.
    	 *
    	 * @since 2.1.0
    	 *
    	 * @global string $text_direction
    	 */
    	public function init() {
    		// SpeedySpec: WordPress's 64 lookups, in its order (served natively when nothing could tell), assigned with
    		// WordPress's own expressions.
    		$t = \SpeedySpec\WpI18n\locale_strings();

    		$this->weekday[0] = $t[0];
    		$this->weekday[1] = $t[1];
    		$this->weekday[2] = $t[2];
    		$this->weekday[3] = $t[3];
    		$this->weekday[4] = $t[4];
    		$this->weekday[5] = $t[5];
    		$this->weekday[6] = $t[6];
    		$this->weekday_initial[ $this->weekday[0] ] = $t[7];
    		$this->weekday_initial[ $this->weekday[1] ] = $t[8];
    		$this->weekday_initial[ $this->weekday[2] ] = $t[9];
    		$this->weekday_initial[ $this->weekday[3] ] = $t[10];
    		$this->weekday_initial[ $this->weekday[4] ] = $t[11];
    		$this->weekday_initial[ $this->weekday[5] ] = $t[12];
    		$this->weekday_initial[ $this->weekday[6] ] = $t[13];
    		$this->weekday_abbrev[ $this->weekday[0] ] = $t[14];
    		$this->weekday_abbrev[ $this->weekday[1] ] = $t[15];
    		$this->weekday_abbrev[ $this->weekday[2] ] = $t[16];
    		$this->weekday_abbrev[ $this->weekday[3] ] = $t[17];
    		$this->weekday_abbrev[ $this->weekday[4] ] = $t[18];
    		$this->weekday_abbrev[ $this->weekday[5] ] = $t[19];
    		$this->weekday_abbrev[ $this->weekday[6] ] = $t[20];
    		$this->month['01'] = $t[21];
    		$this->month['02'] = $t[22];
    		$this->month['03'] = $t[23];
    		$this->month['04'] = $t[24];
    		$this->month['05'] = $t[25];
    		$this->month['06'] = $t[26];
    		$this->month['07'] = $t[27];
    		$this->month['08'] = $t[28];
    		$this->month['09'] = $t[29];
    		$this->month['10'] = $t[30];
    		$this->month['11'] = $t[31];
    		$this->month['12'] = $t[32];
    		$this->month_genitive['01'] = $t[33];
    		$this->month_genitive['02'] = $t[34];
    		$this->month_genitive['03'] = $t[35];
    		$this->month_genitive['04'] = $t[36];
    		$this->month_genitive['05'] = $t[37];
    		$this->month_genitive['06'] = $t[38];
    		$this->month_genitive['07'] = $t[39];
    		$this->month_genitive['08'] = $t[40];
    		$this->month_genitive['09'] = $t[41];
    		$this->month_genitive['10'] = $t[42];
    		$this->month_genitive['11'] = $t[43];
    		$this->month_genitive['12'] = $t[44];
    		$this->month_abbrev[ $this->month['01'] ] = $t[45];
    		$this->month_abbrev[ $this->month['02'] ] = $t[46];
    		$this->month_abbrev[ $this->month['03'] ] = $t[47];
    		$this->month_abbrev[ $this->month['04'] ] = $t[48];
    		$this->month_abbrev[ $this->month['05'] ] = $t[49];
    		$this->month_abbrev[ $this->month['06'] ] = $t[50];
    		$this->month_abbrev[ $this->month['07'] ] = $t[51];
    		$this->month_abbrev[ $this->month['08'] ] = $t[52];
    		$this->month_abbrev[ $this->month['09'] ] = $t[53];
    		$this->month_abbrev[ $this->month['10'] ] = $t[54];
    		$this->month_abbrev[ $this->month['11'] ] = $t[55];
    		$this->month_abbrev[ $this->month['12'] ] = $t[56];
    		$this->meridiem['am'] = $t[57];
    		$this->meridiem['pm'] = $t[58];
    		$this->meridiem['AM'] = $t[59];
    		$this->meridiem['PM'] = $t[60];

    		$thousands_sep = $t[61];
    		// Replace space with a non-breaking space to avoid wrapping.
    		$thousands_sep = str_replace( ' ', '&nbsp;', $thousands_sep );

    		$this->number_format['thousands_sep'] = ( 'number_format_thousands_sep' === $thousands_sep ) ? ',' : $thousands_sep;

    		$decimal_point = $t[62];

    		$this->number_format['decimal_point'] = ( 'number_format_decimal_point' === $decimal_point ) ? '.' : $decimal_point;

    		$this->list_item_separator = $t[63];

    		// Set text direction.
    		if ( isset( $GLOBALS['text_direction'] ) ) {
    			$this->text_direction = $GLOBALS['text_direction'];

    		} elseif ( 'rtl' === $t[64] ) {
    			$this->text_direction = 'rtl';
    		}

    		// Set the word count type.
    		$this->word_count_type = $this->get_word_count_type();
    	}

    	/**
    	 * Retrieves the full translated weekday word.
    	 *
    	 * Week starts on translated Sunday and can be fetched
    	 * by using 0 (zero). So the week starts with 0 (zero)
    	 * and ends on Saturday with is fetched by using 6 (six).
    	 *
    	 * @since 2.1.0
    	 *
    	 * @param int $weekday_number 0 for Sunday through 6 Saturday.
    	 * @return string Full translated weekday.
    	 */
    	public function get_weekday( $weekday_number ) {
    		return $this->weekday[ $weekday_number ];
    	}

    	/**
    	 * Retrieves the translated weekday initial.
    	 *
    	 * The weekday initial is retrieved by the translated
    	 * full weekday word. When translating the weekday initial
    	 * pay attention to make sure that the starting letter does
    	 * not conflict.
    	 *
    	 * @since 2.1.0
    	 *
    	 * @param string $weekday_name Full translated weekday word.
    	 * @return string Translated weekday initial.
    	 */
    	public function get_weekday_initial( $weekday_name ) {
    		return $this->weekday_initial[ $weekday_name ];
    	}

    	/**
    	 * Retrieves the translated weekday abbreviation.
    	 *
    	 * The weekday abbreviation is retrieved by the translated
    	 * full weekday word.
    	 *
    	 * @since 2.1.0
    	 *
    	 * @param string $weekday_name Full translated weekday word.
    	 * @return string Translated weekday abbreviation.
    	 */
    	public function get_weekday_abbrev( $weekday_name ) {
    		return $this->weekday_abbrev[ $weekday_name ];
    	}

    	/**
    	 * Retrieves the full translated month by month number.
    	 *
    	 * The $month_number parameter has to be a string
    	 * because it must have the '0' in front of any number
    	 * that is less than 10. Starts from '01' and ends at
    	 * '12'.
    	 *
    	 * You can use an integer instead and it will add the
    	 * '0' before the numbers less than 10 for you.
    	 *
    	 * @since 2.1.0
    	 *
    	 * @param string|int $month_number '01' through '12'.
    	 * @return string Translated full month name. If the month number is not found, an empty string is returned.
    	 */
    	public function get_month( $month_number ) {
    		$month_number = zeroise( $month_number, 2 );
    		if ( ! isset( $this->month[ $month_number ] ) ) {
    			return '';
    		}
    		return $this->month[ $month_number ];
    	}

    	/**
    	 * Retrieves translated version of month abbreviation string.
    	 *
    	 * The $month_name parameter is expected to be the translated or
    	 * translatable version of the month.
    	 *
    	 * @since 2.1.0
    	 *
    	 * @param string $month_name Translated month to get abbreviated version.
    	 * @return string Translated abbreviated month.
    	 */
    	public function get_month_abbrev( $month_name ) {
    		return $this->month_abbrev[ $month_name ];
    	}

    	/**
    	 * Retrieves translated version of month genitive string.
    	 *
    	 * The $month_number parameter has to be a string
    	 * because it must have the '0' in front of any number
    	 * that is less than 10. Starts from '01' and ends at
    	 * '12'.
    	 *
    	 * You can use an integer instead and it will add the
    	 * '0' before the numbers less than 10 for you.
    	 *
    	 * @since 6.8.0
    	 *
    	 * @param string|int $month_number '01' through '12'.
    	 * @return string Translated genitive month name.
    	 */
    	public function get_month_genitive( $month_number ) {
    		return $this->month_genitive[ zeroise( $month_number, 2 ) ];
    	}

    	/**
    	 * Retrieves translated version of meridiem string.
    	 *
    	 * The $meridiem parameter is expected to not be translated.
    	 *
    	 * @since 2.1.0
    	 *
    	 * @param string $meridiem Either 'am', 'pm', 'AM', or 'PM'. Not translated version.
    	 * @return string Translated version
    	 */
    	public function get_meridiem( $meridiem ) {
    		return $this->meridiem[ $meridiem ];
    	}

    	/**
    	 * Global variables are deprecated.
    	 *
    	 * For backward compatibility only.
    	 *
    	 * @since 2.1.0
    	 * @deprecated For backward compatibility only.
    	 *
    	 * @global array $weekday
    	 * @global array $weekday_initial
    	 * @global array $weekday_abbrev
    	 * @global array $month
    	 * @global array $month_abbrev
    	 */
    	public function register_globals() {
    		$GLOBALS['weekday']         = $this->weekday;
    		$GLOBALS['weekday_initial'] = $this->weekday_initial;
    		$GLOBALS['weekday_abbrev']  = $this->weekday_abbrev;
    		$GLOBALS['month']           = $this->month;
    		$GLOBALS['month_abbrev']    = $this->month_abbrev;
    	}

    	/**
    	 * Checks if current locale is RTL.
    	 *
    	 * @since 3.0.0
    	 * @return bool Whether locale is RTL.
    	 */
    	public function is_rtl() {
    		return 'rtl' === $this->text_direction;
    	}

    	/**
    	 * Registers date/time format strings for general POT.
    	 *
    	 * Private, unused method to add some date/time formats translated
    	 * on wp-admin/options-general.php to the general POT that would
    	 * otherwise be added to the admin POT.
    	 *
    	 * @since 3.6.0
    	 */
    	public function _strings_for_pot() {
    		/* translators: Localized date format, see https://www.php.net/manual/datetime.format.php */
    		__( 'F j, Y' );
    		/* translators: Localized time format, see https://www.php.net/manual/datetime.format.php */
    		__( 'g:i a' );
    		/* translators: Localized date and time format, see https://www.php.net/manual/datetime.format.php */
    		__( 'F j, Y g:i a' );
    	}

    	/**
    	 * Retrieves the localized list item separator.
    	 *
    	 * @since 6.0.0
    	 *
    	 * @return string Localized list item separator.
    	 */
    	public function get_list_item_separator() {
    		return $this->list_item_separator;
    	}

    	/**
    	 * Retrieves the localized word count type.
    	 *
    	 * @since 6.2.0
    	 *
    	 * @return string Localized word count type. Possible values are `characters_excluding_spaces`,
    	 *                `characters_including_spaces`, or `words`. Defaults to `words`.
    	 */
    	public function get_word_count_type() {

    		/*
    		 * translators: If your word count is based on single characters (e.g. East Asian characters),
    		 * enter 'characters_excluding_spaces' or 'characters_including_spaces'. Otherwise, enter 'words'.
    		 * Do not translate into your own language.
    		 */
    		$word_count_type = $this->word_count_type ?? _x( 'words', 'Word count type. Do not translate!' );

    		// Check for valid types.
    		if ( 'characters_excluding_spaces' !== $word_count_type && 'characters_including_spaces' !== $word_count_type ) {
    			// Defaults to 'words'.
    			$word_count_type = 'words';
    		}

    		return $word_count_type;
    	}
    }

    // WP_Locale_Switcher is WordPress 7.1.5's own class (GPL-2.0-or-later) over the native switcher; the work of its
    // private load_translations() and change_locale() is SpeedySpec\WpI18n\switcher_change_locale().
    /**
     * Core class used for switching locales.
     *
     * @since 4.7.0
     */
    #[AllowDynamicProperties]
    class WP_Locale_Switcher {
    	/**
    	 * Locale switching stack.
    	 *
    	 * @since 6.2.0
    	 * @var array
    	 */
    	private $stack = array();

    	/**
    	 * Original locale.
    	 *
    	 * @since 4.7.0
    	 * @var string
    	 */
    	private $original_locale;

    	/**
    	 * Holds all available languages.
    	 *
    	 * @since 4.7.0
    	 * @var string[] An array of language codes (file names without the .mo extension).
    	 */
    	private $available_languages;

    	/**
    	 * Constructor.
    	 *
    	 * Stores the original locale as well as a list of all available languages.
    	 *
    	 * @since 4.7.0
    	 */
    	public function __construct() {
    		$this->original_locale     = determine_locale();
    		$this->available_languages = array_merge( array( 'en_US' ), get_available_languages() );
    		// SpeedySpec: the stack and its decisions are native.
    		\SpeedySpec\WpI18n\Native\switcher_new( $this, $this->original_locale, $this->available_languages );
    	}

    	/**
    	 * Initializes the locale switcher.
    	 *
    	 * Hooks into the {@see 'locale'} and {@see 'determine_locale'} filters
    	 * to change the locale on the fly.
    	 *
    	 * @since 4.7.0
    	 */
    	public function init() {
    		add_filter( 'locale', array( $this, 'filter_locale' ) );
    		add_filter( 'determine_locale', array( $this, 'filter_locale' ) );
    	}

    	/**
    	 * Switches the translations according to the given locale.
    	 *
    	 * @since 4.7.0
    	 *
    	 * @param string    $locale  The locale to switch to.
    	 * @param int|false $user_id Optional. User ID as context. Default false.
    	 * @return bool True on success, false on failure.
    	 */
    	public function switch_to_locale( $locale, $user_id = false ) {
    		$switched    = \SpeedySpec\WpI18n\Native\switcher_switch( $this, $locale, $user_id );
    		$this->stack = \SpeedySpec\WpI18n\Native\switcher_stack( $this );
    		return $switched;
    	}

    	/**
    	 * Switches the translations according to the given user's locale.
    	 *
    	 * @since 6.2.0
    	 *
    	 * @param int $user_id User ID.
    	 * @return bool True on success, false on failure.
    	 */
    	public function switch_to_user_locale( $user_id ) {
    		$switched    = \SpeedySpec\WpI18n\Native\switcher_switch_user( $this, $user_id );
    		$this->stack = \SpeedySpec\WpI18n\Native\switcher_stack( $this );
    		return $switched;
    	}

    	/**
    	 * Restores the translations according to the previous locale.
    	 *
    	 * @since 4.7.0
    	 *
    	 * @return string|false Locale on success, false on failure.
    	 */
    	public function restore_previous_locale() {
    		$locale      = \SpeedySpec\WpI18n\Native\switcher_restore( $this );
    		$this->stack = \SpeedySpec\WpI18n\Native\switcher_stack( $this );
    		return $locale;
    	}

    	/**
    	 * Restores the translations according to the original locale.
    	 *
    	 * @since 4.7.0
    	 *
    	 * @return string|false Locale on success, false on failure.
    	 */
    	public function restore_current_locale() {
    		$locale      = \SpeedySpec\WpI18n\Native\switcher_restore_current( $this );
    		$this->stack = \SpeedySpec\WpI18n\Native\switcher_stack( $this );
    		return $locale;
    	}

    	/**
    	 * Whether switch_to_locale() is in effect.
    	 *
    	 * @since 4.7.0
    	 *
    	 * @return bool True if the locale has been switched, false otherwise.
    	 */
    	public function is_switched() {
    		return \SpeedySpec\WpI18n\Native\switcher_is_switched( $this );
    	}

    	/**
    	 * Returns the locale currently switched to.
    	 *
    	 * @since 6.2.0
    	 *
    	 * @return string|false Locale if the locale has been switched, false otherwise.
    	 */
    	public function get_switched_locale() {
    		return \SpeedySpec\WpI18n\Native\switcher_locale( $this );
    	}

    	/**
    	 * Returns the user ID related to the currently switched locale.
    	 *
    	 * @since 6.2.0
    	 *
    	 * @return int|false User ID if set and if the locale has been switched, false otherwise.
    	 */
    	public function get_switched_user_id() {
    		return \SpeedySpec\WpI18n\Native\switcher_user_id( $this );
    	}

    	/**
    	 * Filters the locale of the WordPress installation.
    	 *
    	 * @since 4.7.0
    	 *
    	 * @param string $locale The locale of the WordPress installation.
    	 * @return string The locale currently being switched to.
    	 */
    	public function filter_locale( $locale ) {
    		return \SpeedySpec\WpI18n\Native\switcher_filter( $this, $locale );
    	}

    }

    // Plural_Forms is WordPress 7.1.5's own class (GPL-2.0-or-later) with parse() and, for an integer, execute()
    // native.
    /**
     * A gettext Plural-Forms parser.
     *
     * @since 4.9.0
     */
    #[AllowDynamicProperties]
    class Plural_Forms {
    	/**
    	 * Operator characters.
    	 *
    	 * @since 4.9.0
    	 * @var string OP_CHARS Operator characters.
    	 */
    	const OP_CHARS = '|&><!=%?:';

    	/**
    	 * Valid number characters.
    	 *
    	 * @since 4.9.0
    	 * @var string NUM_CHARS Valid number characters.
    	 */
    	const NUM_CHARS = '0123456789';

    	/**
    	 * Operator precedence.
    	 *
    	 * Operator precedence from highest to lowest. Higher numbers indicate
    	 * higher precedence, and are executed first.
    	 *
    	 * @see https://en.wikipedia.org/wiki/Operators_in_C_and_C%2B%2B#Operator_precedence
    	 *
    	 * @since 4.9.0
    	 * @var array $op_precedence Operator precedence from highest to lowest.
    	 */
    	protected static $op_precedence = array(
    		'%'  => 6,

    		'<'  => 5,
    		'<=' => 5,
    		'>'  => 5,
    		'>=' => 5,

    		'==' => 4,
    		'!=' => 4,

    		'&&' => 3,

    		'||' => 2,

    		'?:' => 1,
    		'?'  => 1,

    		'('  => 0,
    		')'  => 0,
    	);

    	/**
    	 * Tokens generated from the string.
    	 *
    	 * @since 4.9.0
    	 * @var array $tokens List of tokens.
    	 */
    	protected $tokens = array();

    	/**
    	 * Cache for repeated calls to the function.
    	 *
    	 * @since 4.9.0
    	 * @var array $cache Map of $n => $result
    	 */
    	protected $cache = array();

    	/**
    	 * Constructor.
    	 *
    	 * @since 4.9.0
    	 *
    	 * @param string $str Plural function (just the bit after `plural=` from Plural-Forms)
    	 */
    	public function __construct( $str ) {
    		$this->parse( $str );
    	}

    	/**
    	 * Parse a Plural-Forms string into tokens.
    	 *
    	 * Uses the shunting-yard algorithm to convert the string to Reverse Polish
    	 * Notation tokens.
    	 *
    	 * @since 4.9.0
    	 *
    	 * @throws Exception If there is a syntax or parsing error with the string.
    	 *
    	 * @param string $str String to parse.
    	 */
    	protected function parse( $str ) {
    		// SpeedySpec: WordPress's shunting-yard parse, natively, into WordPress's token list.
    		$this->tokens = \SpeedySpec\WpI18n\Native\plural_parse( (string) $str );
    	}

    	/**
    	 * Get the plural form for a number.
    	 *
    	 * Caches the value for repeated calls.
    	 *
    	 * @since 4.9.0
    	 *
    	 * @param int $num Number to get plural form for.
    	 * @return int Plural form value.
    	 */
    	public function get( $num ) {
    		if ( isset( $this->cache[ $num ] ) ) {
    			return $this->cache[ $num ];
    		}
    		$this->cache[ $num ] = $this->execute( $num );
    		return $this->cache[ $num ];
    	}

    	/**
    	 * Execute the plural form function.
    	 *
    	 * @since 4.9.0
    	 *
    	 * @throws Exception If the plural form value cannot be calculated.
    	 *
    	 * @param int $n Variable "n" to substitute.
    	 * @return int Plural form value.
    	 */
    	public function execute( $n ) {
    		// SpeedySpec: an integer runs the object's own tokens natively; anything else runs WordPress's body.
    		if ( is_int( $n ) ) {
    			$result = \SpeedySpec\WpI18n\Native\plural_execute( $this->tokens, $n );
    			if ( null !== $result ) {
    				return $result;
    			}
    		}

    		$stack = array();
    		$i     = 0;
    		$total = count( $this->tokens );
    		while ( $i < $total ) {
    			$next = $this->tokens[ $i ];
    			++$i;
    			if ( 'var' === $next[0] ) {
    				$stack[] = $n;
    				continue;
    			} elseif ( 'value' === $next[0] ) {
    				$stack[] = $next[1];
    				continue;
    			}

    			// Only operators left.
    			switch ( $next[1] ) {
    				case '%':
    					$v2      = array_pop( $stack );
    					$v1      = array_pop( $stack );
    					$stack[] = $v1 % $v2;
    					break;

    				case '||':
    					$v2      = array_pop( $stack );
    					$v1      = array_pop( $stack );
    					$stack[] = $v1 || $v2;
    					break;

    				case '&&':
    					$v2      = array_pop( $stack );
    					$v1      = array_pop( $stack );
    					$stack[] = $v1 && $v2;
    					break;

    				case '<':
    					$v2      = array_pop( $stack );
    					$v1      = array_pop( $stack );
    					$stack[] = $v1 < $v2;
    					break;

    				case '<=':
    					$v2      = array_pop( $stack );
    					$v1      = array_pop( $stack );
    					$stack[] = $v1 <= $v2;
    					break;

    				case '>':
    					$v2      = array_pop( $stack );
    					$v1      = array_pop( $stack );
    					$stack[] = $v1 > $v2;
    					break;

    				case '>=':
    					$v2      = array_pop( $stack );
    					$v1      = array_pop( $stack );
    					$stack[] = $v1 >= $v2;
    					break;

    				case '!=':
    					$v2      = array_pop( $stack );
    					$v1      = array_pop( $stack );
    					$stack[] = $v1 !== $v2;
    					break;

    				case '==':
    					$v2      = array_pop( $stack );
    					$v1      = array_pop( $stack );
    					$stack[] = $v1 === $v2;
    					break;

    				case '?:':
    					$v3      = array_pop( $stack );
    					$v2      = array_pop( $stack );
    					$v1      = array_pop( $stack );
    					$stack[] = $v1 ? $v2 : $v3;
    					break;

    				default:
    					throw new Exception( sprintf( 'Unknown operator "%s"', $next[1] ) );
    			}
    		}

    		if ( count( $stack ) !== 1 ) {
    			throw new Exception( 'Too many values remaining on the stack' );
    		}

    		return (int) $stack[0];
    	}
    }
}
