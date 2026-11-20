declare(strict_types=1);
namespace SpeedySpec\WpDb
{
    /**
     * Stands in for `$wpdb->dbh` while the request holds a connection.
     *
     *  The connection belongs to the engine and is not a `mysqli`; code that passes `$wpdb->dbh` to a `mysqli_*`
     *  function fails (the documented break `dbh_is_not_mysqli`).
     */
    final class Connection
    {
    }

    /**
     * Keeps `wpdb`'s per-request state where a host wants it, in place of the worker's memory.
     *
     *  Bound in the container under `SpeedySpec\WpDb\WpdbState`, an implementation receives the whole state after
     *  each change and hands it back on each read.
     */
    interface WpdbState
    {
        public function load(): array;

        public function store(array $state): void;

        public function reset(): void;
    }
}
namespace
{
    use SpeedySpec\WpDb\Native as N;
    if (!defined('EZSQL_VERSION'))
    {
        define('EZSQL_VERSION', 'WP1.25');
    }
    if (!defined('OBJECT'))
    {
        define('OBJECT', 'OBJECT');
    }
    if (!defined('object'))
    {
        define('object', 'OBJECT');
    }
    if (!defined('OBJECT_K'))
    {
        define('OBJECT_K', 'OBJECT_K');
    }
    if (!defined('ARRAY_A'))
    {
        define('ARRAY_A', 'ARRAY_A');
    }
    if (!defined('ARRAY_N'))
    {
        define('ARRAY_N', 'ARRAY_N');
    }

    /**
     * Gives WordPress code its database access object, native over SpeedySpec's Rust port of `wpdb`.
     *
     *  Every method keeps WordPress's name, parameters and defaults and hands the call to the port, where every
     *  rule of `wpdb` lives; this class only converts results, such as rows into `stdClass` objects.
     */
    #[AllowDynamicProperties]
    class wpdb
    {
        private int $speedyspec_db = 0;
        private int $speedyspec_generation = -1;
        private array $speedyspec_objects = [];
        private ?\SpeedySpec\WpDb\Connection $speedyspec_connection = null;
        private array $speedyspec_converted = [];

        public $show_errors
        {
            get => $this->speedyspec_get('show_errors');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'show_errors', $value);
            }
        }

        public $suppress_errors
        {
            get => $this->speedyspec_get('suppress_errors');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'suppress_errors', $value);
            }
        }

        public $last_error
        {
            get => $this->speedyspec_get('last_error');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'last_error', $value);
            }
        }

        public $num_queries
        {
            get => $this->speedyspec_get('num_queries');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'num_queries', $value);
            }
        }

        public $num_rows
        {
            get => $this->speedyspec_get('num_rows');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'num_rows', $value);
            }
        }

        public $rows_affected
        {
            get => $this->speedyspec_get('rows_affected');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'rows_affected', $value);
            }
        }

        public $insert_id
        {
            get => $this->speedyspec_get('insert_id');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'insert_id', $value);
            }
        }

        public $last_query
        {
            get => $this->speedyspec_get('last_query');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'last_query', $value);
            }
        }

        public $queries
        {
            get => $this->speedyspec_get('queries');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'queries', $value);
            }
        }

        public $prefix
        {
            get => $this->speedyspec_get('prefix');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'prefix', $value);
            }
        }

        public $base_prefix
        {
            get => $this->speedyspec_get('base_prefix');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'base_prefix', $value);
            }
        }

        public $ready
        {
            get => $this->speedyspec_get('ready');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'ready', $value);
            }
        }

        public $blogid
        {
            get => $this->speedyspec_get('blogid');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'blogid', $value);
            }
        }

        public $siteid
        {
            get => $this->speedyspec_get('siteid');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'siteid', $value);
            }
        }

        public $tables
        {
            get => $this->speedyspec_get('tables');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'tables', $value);
            }
        }

        public $old_tables
        {
            get => $this->speedyspec_get('old_tables');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'old_tables', $value);
            }
        }

        public $global_tables
        {
            get => $this->speedyspec_get('global_tables');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'global_tables', $value);
            }
        }

        public $ms_global_tables
        {
            get => $this->speedyspec_get('ms_global_tables');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'ms_global_tables', $value);
            }
        }

        public $old_ms_global_tables
        {
            get => $this->speedyspec_get('old_ms_global_tables');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'old_ms_global_tables', $value);
            }
        }

        public $comments
        {
            get => $this->speedyspec_get('comments');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'comments', $value);
            }
        }

        public $commentmeta
        {
            get => $this->speedyspec_get('commentmeta');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'commentmeta', $value);
            }
        }

        public $links
        {
            get => $this->speedyspec_get('links');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'links', $value);
            }
        }

        public $options
        {
            get => $this->speedyspec_get('options');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'options', $value);
            }
        }

        public $postmeta
        {
            get => $this->speedyspec_get('postmeta');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'postmeta', $value);
            }
        }

        public $posts
        {
            get => $this->speedyspec_get('posts');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'posts', $value);
            }
        }

        public $terms
        {
            get => $this->speedyspec_get('terms');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'terms', $value);
            }
        }

        public $term_relationships
        {
            get => $this->speedyspec_get('term_relationships');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'term_relationships', $value);
            }
        }

        public $term_taxonomy
        {
            get => $this->speedyspec_get('term_taxonomy');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'term_taxonomy', $value);
            }
        }

        public $termmeta
        {
            get => $this->speedyspec_get('termmeta');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'termmeta', $value);
            }
        }

        public $usermeta
        {
            get => $this->speedyspec_get('usermeta');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'usermeta', $value);
            }
        }

        public $users
        {
            get => $this->speedyspec_get('users');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'users', $value);
            }
        }

        public $blogs
        {
            get => $this->speedyspec_get('blogs');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'blogs', $value);
            }
        }

        public $blogmeta
        {
            get => $this->speedyspec_get('blogmeta');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'blogmeta', $value);
            }
        }

        public $registration_log
        {
            get => $this->speedyspec_get('registration_log');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'registration_log', $value);
            }
        }

        public $signups
        {
            get => $this->speedyspec_get('signups');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'signups', $value);
            }
        }

        public $site
        {
            get => $this->speedyspec_get('site');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'site', $value);
            }
        }

        public $sitecategories
        {
            get => $this->speedyspec_get('sitecategories');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'sitecategories', $value);
            }
        }

        public $sitemeta
        {
            get => $this->speedyspec_get('sitemeta');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'sitemeta', $value);
            }
        }

        public $field_types
        {
            get => $this->speedyspec_get('field_types');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'field_types', $value);
            }
        }

        public $charset
        {
            get => $this->speedyspec_get('charset');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'charset', $value);
            }
        }

        public $collate
        {
            get => $this->speedyspec_get('collate');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'collate', $value);
            }
        }

        public $func_call
        {
            get => $this->speedyspec_get('func_call');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'func_call', $value);
            }
        }

        public $time_start
        {
            get => $this->speedyspec_get('time_start');
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'time_start', $value);
            }
        }

        private $allow_unsafe_unquoted_parameters
        {
            get => $this->speedyspec_get('allow_unsafe_unquoted_parameters');
            set($value)
            {
                N\set_allow_unsafe($this->speedyspec_db, $value);
            }
        }

        public $last_result
        {
            get => $this->speedyspec_last_result();
            set($value)
            {
                // The rows' properties go to the port; the objects themselves are handed out until the next result.
                $value = is_array($value) ? $value : array();
                $rows = array_map(static fn ($row) => is_object($row) ? get_object_vars($row) : (array) $row, $value);
                N\prop_set($this->speedyspec_db, 'last_result', $rows);
                [$this->speedyspec_generation] = N\last_result($this->speedyspec_db, -1);
                $this->speedyspec_objects = array_values($value);
            }
        }

        public $error
        {
            get => $this->speedyspec_error();
            set($value)
            {
                N\prop_set($this->speedyspec_db, 'error', $value);
            }
        }

        public function __construct($dbuser, #[\SensitiveParameter] $dbpassword, $dbname, $dbhost)
        {
            $this->speedyspec_db = N\open($dbuser, $dbpassword, $dbname, $dbhost);
            N\construct($this->speedyspec_db);
        }

        /**
         * Reads a property through the port, keeping PHP's undefined-property result for unknown names.
         */
        private function speedyspec_get(string $name): mixed
        {
            $found = N\prop_get($this->speedyspec_db, $name);
            return 'ok' === $found[0] ? $found[1] : null;
        }

        /**
         * Hands out the rows of the last result as objects, made once per result so they stay identical.
         */
        private function speedyspec_last_result(): array
        {
            [$generation, $rows] = N\last_result($this->speedyspec_db, $this->speedyspec_generation);
            if (null !== $rows)
            {
                $this->speedyspec_generation = $generation;
                $this->speedyspec_objects = array_map(static fn (array $row): object => (object) $row, $rows);
            }
            return $this->speedyspec_objects;
        }

        /**
         * Turns the port's error marker into the `WP_Error` WordPress returns.
         */
        private function speedyspec_result(mixed $result): mixed
        {
            if (is_array($result) && isset($result['__speedyspec_error']))
            {
                return class_exists('WP_Error', false) ? new \WP_Error($result['code'], $result['message']) : false;
            }
            return $result;
        }

        private function speedyspec_error(): mixed
        {
            $error = $this->speedyspec_get('error');
            if (!is_array($error))
            {
                return $error;
            }
            if ('' === $error[0] || !class_exists('WP_Error', false))
            {
                return $error[1];
            }
            return new \WP_Error($error[0], $error[1]);
        }

        private function speedyspec_hidden(string $name): mixed
        {
            switch ($name)
            {
                case 'dbh':
                    if (!$this->speedyspec_get('dbh'))
                    {
                        return null;
                    }
                    return $this->speedyspec_connection ??= new \SpeedySpec\WpDb\Connection();
                case 'col_info':
                    return $this->speedyspec_converted('col_info', static fn (array $cols): array => array_map(
                        static fn (array $c): object => (object) $c,
                        $cols
                    ));
                case 'col_meta':
                    return $this->speedyspec_converted('col_meta', static fn (array $tables): array => array_map(
                        static fn (array $cols): array => array_map(static fn (array $c): object => (object) $c, $cols),
                        $tables
                    ));
                case 'result':
                    return null;
                default:
                    return $this->speedyspec_get($name);
            }
        }

        /**
         * Converts a property's arrays into objects once, so repeated reads hand out the same objects.
         */
        private function speedyspec_converted(string $name, \Closure $convert): mixed
        {
            $raw = $this->speedyspec_get($name);
            if (null === $raw)
            {
                return null;
            }
            if (!isset($this->speedyspec_converted[$name]) || $this->speedyspec_converted[$name][0] !== $raw)
            {
                $this->speedyspec_converted[$name] = [$raw, $convert($raw)];
            }
            return $this->speedyspec_converted[$name][1];
        }

        public function __get($name)
        {
            $found = N\prop_get($this->speedyspec_db, (string) $name);
            if ('ok' !== $found[0])
            {
                trigger_error('Undefined property: wpdb::$' . $name, E_USER_WARNING);
                return null;
            }
            return $this->speedyspec_hidden((string) $name);
        }

        public function __set($name, $value)
        {
            if (!N\prop_set($this->speedyspec_db, (string) $name, $value))
            {
                $this->$name = $value;
            }
        }

        public function __isset($name)
        {
            return null !== $this->speedyspec_hidden((string) $name);
        }

        public function __unset($name)
        {
            N\prop_set($this->speedyspec_db, (string) $name, null);
        }

        public function init_charset()
        {
            N\init_charset($this->speedyspec_db);
        }

        public function determine_charset($charset, $collate)
        {
            return N\determine_charset($this->speedyspec_db, $charset, $collate);
        }

        public function set_charset($dbh, $charset = null, $collate = null)
        {
            N\set_charset($this->speedyspec_db, $charset, $collate);
        }

        public function set_sql_mode($modes = array())
        {
            N\set_sql_mode($this->speedyspec_db, $modes);
        }

        public function set_prefix($prefix, $set_table_names = true)
        {
            return $this->speedyspec_result(N\set_prefix($this->speedyspec_db, $prefix, $set_table_names));
        }

        public function set_blog_id($blog_id, $network_id = 0)
        {
            return N\set_blog_id($this->speedyspec_db, $blog_id, $network_id);
        }

        public function get_blog_prefix($blog_id = null)
        {
            return N\get_blog_prefix($this->speedyspec_db, $blog_id);
        }

        public function tables($scope = 'all', $prefix = true, $blog_id = 0)
        {
            return N\tables($this->speedyspec_db, $scope, $prefix, $blog_id);
        }

        public function select($db, $dbh = null)
        {
            N\select($this->speedyspec_db, $db);
        }

        public function _weak_escape($data)
        {
            return N\weak_escape($this->speedyspec_db, $data, func_num_args() === 1 && function_exists('_deprecated_function'), __METHOD__);
        }

        public function _real_escape($data)
        {
            return N\real_escape($this->speedyspec_db, $data);
        }

        public function _escape($data)
        {
            return N\escape_data($this->speedyspec_db, $data);
        }

        public function escape($data)
        {
            return N\weak_escape($this->speedyspec_db, $data, func_num_args() === 1 && function_exists('_deprecated_function'), __METHOD__);
        }

        public function escape_by_ref(&$data)
        {
            if (!is_float($data))
            {
                $data = $this->_real_escape($data);
            }
        }

        public function quote_identifier($identifier)
        {
            return N\quote_identifier($this->speedyspec_db, $identifier);
        }

        public function prepare($query, ...$args)
        {
            return N\prepare($this->speedyspec_db, $query, ...$args);
        }

        public function esc_like($text)
        {
            return N\esc_like($this->speedyspec_db, $text);
        }

        public function print_error($str = '')
        {
            return N\print_error($this->speedyspec_db, $str);
        }

        public function show_errors($show = true)
        {
            return N\show_errors($this->speedyspec_db, $show);
        }

        public function hide_errors()
        {
            return N\hide_errors($this->speedyspec_db);
        }

        public function suppress_errors($suppress = true)
        {
            return N\suppress_errors($this->speedyspec_db, $suppress);
        }

        public function flush()
        {
            N\flush($this->speedyspec_db);
        }

        public function db_connect($allow_bail = true)
        {
            return N\db_connect($this->speedyspec_db, $allow_bail);
        }

        public function parse_db_host($host)
        {
            return N\parse_db_host($this->speedyspec_db, $host);
        }

        public function check_connection($allow_bail = true)
        {
            return N\check_connection($this->speedyspec_db, $allow_bail);
        }

        public function query($query)
        {
            return N\query($this->speedyspec_db, $query);
        }

        public function log_query($query, $query_time, $query_callstack, $query_start, $query_data)
        {
            N\log_query($this->speedyspec_db, $query, $query_time, $query_callstack, $query_start, $query_data);
        }

        public function placeholder_escape()
        {
            return N\placeholder_escape($this->speedyspec_db);
        }

        public function add_placeholder_escape($query)
        {
            return N\add_placeholder_escape($this->speedyspec_db, $query);
        }

        public function remove_placeholder_escape($query)
        {
            return N\remove_placeholder_escape($this->speedyspec_db, $query);
        }

        public function insert($table, $data, $format = null)
        {
            return N\insert($this->speedyspec_db, $table, $data, $format);
        }

        public function replace($table, $data, $format = null)
        {
            return N\replace($this->speedyspec_db, $table, $data, $format);
        }

        public function _insert_replace_helper($table, $data, $format = null, $type = 'INSERT')
        {
            return N\insert_replace_helper($this->speedyspec_db, $table, $data, $format, $type);
        }

        public function update($table, $data, $where, $format = null, $where_format = null)
        {
            return N\update($this->speedyspec_db, $table, $data, $where, $format, $where_format);
        }

        public function delete($table, $where, $where_format = null)
        {
            return N\delete($this->speedyspec_db, $table, $where, $where_format);
        }

        protected function process_fields($table, $data, $format)
        {
            return N\process_fields($this->speedyspec_db, $table, $data, $format);
        }

        protected function process_field_formats($data, $format)
        {
            return N\process_field_formats($this->speedyspec_db, $data, $format);
        }

        protected function process_field_charsets($data, $table)
        {
            return N\process_field_charsets($this->speedyspec_db, $data, $table);
        }

        protected function process_field_lengths($data, $table)
        {
            return N\process_field_lengths($this->speedyspec_db, $data, $table);
        }

        public function get_var($query = null, $x = 0, $y = 0)
        {
            return N\get_var($this->speedyspec_db, $query, $x, $y);
        }

        public function get_row($query = null, $output = OBJECT, $y = 0)
        {
            $row = N\get_row($this->speedyspec_db, $query, $output, $y);
            if (is_array($row) && [0, 1] === array_keys($row) && 'o' === $row[0] && is_int($row[1]))
            {
                return $this->speedyspec_last_result()[$row[1]] ?? null;
            }
            return $row;
        }

        public function get_col($query = null, $x = 0)
        {
            return N\get_col($this->speedyspec_db, $query, $x);
        }

        public function get_results($query = null, $output = OBJECT)
        {
            $results = N\get_results($this->speedyspec_db, $query, $output);
            if (null === $results)
            {
                return null;
            }
            if ('o' === $results[0])
            {
                return $this->speedyspec_last_result();
            }
            if ('k' === $results[0])
            {
                $objects = $this->speedyspec_last_result();
                $keyed = [];
                foreach ($results[1] as [$key, $index])
                {
                    $keyed[$key] = $objects[$index];
                }
                return $keyed;
            }
            return $results[1];
        }

        protected function get_table_charset($table)
        {
            return $this->speedyspec_result(N\get_table_charset($this->speedyspec_db, $table));
        }

        public function get_col_charset($table, $column)
        {
            return $this->speedyspec_result(N\get_col_charset($this->speedyspec_db, $table, $column));
        }

        public function get_col_length($table, $column)
        {
            return $this->speedyspec_result(N\get_col_length($this->speedyspec_db, $table, $column));
        }

        protected function check_ascii($input_string)
        {
            return N\check_ascii($this->speedyspec_db, $input_string);
        }

        protected function check_safe_collation($query)
        {
            return N\check_safe_collation($this->speedyspec_db, $query);
        }

        protected function strip_invalid_text($data)
        {
            return $this->speedyspec_result(N\strip_invalid_text($this->speedyspec_db, $data));
        }

        protected function strip_invalid_text_from_query($query)
        {
            return $this->speedyspec_result(N\strip_invalid_text_from_query($this->speedyspec_db, $query));
        }

        public function strip_invalid_text_for_column($table, $column, $value)
        {
            return $this->speedyspec_result(N\strip_invalid_text_for_column($this->speedyspec_db, $table, $column, $value));
        }

        protected function get_table_from_query($query)
        {
            return N\get_table_from_query($this->speedyspec_db, $query);
        }

        protected function load_col_info()
        {
            return;
        }

        public function get_col_info($info_type = 'name', $col_offset = -1)
        {
            return N\get_col_info($this->speedyspec_db, $info_type, $col_offset);
        }

        public function timer_start()
        {
            return N\timer_start($this->speedyspec_db);
        }

        public function timer_stop()
        {
            return N\timer_stop($this->speedyspec_db);
        }

        public function bail($message, $error_code = '500')
        {
            return N\bail($this->speedyspec_db, $message, $error_code);
        }

        public function close()
        {
            return N\close($this->speedyspec_db);
        }

        public function check_database_version()
        {
            $wp_version = function_exists('wp_get_wp_version') ? wp_get_wp_version() : ($GLOBALS['wp_version'] ?? '');
            return $this->speedyspec_result(N\check_database_version($this->speedyspec_db, (string) ($GLOBALS['required_mysql_version'] ?? ''), (string) $wp_version));
        }

        public function supports_collation()
        {
            return N\supports_collation($this->speedyspec_db);
        }

        public function get_charset_collate()
        {
            return N\get_charset_collate($this->speedyspec_db);
        }

        public function has_cap($db_cap)
        {
            return N\has_cap($this->speedyspec_db, $db_cap);
        }

        public function get_caller()
        {
            return N\get_caller($this->speedyspec_db);
        }

        public function db_version()
        {
            return N\db_version($this->speedyspec_db);
        }

        public function db_server_info()
        {
            return N\db_server_info($this->speedyspec_db);
        }
    }
}
