/**
 * WordPress's rewrite API, declared at the start of every request, over the Rust port.
 *
 * WP_Rewrite keeps every public property WordPress declares, with WordPress's defaults, because plugins write them
 * directly (including through indirect array writes such as `$wp_rewrite->extra_rules_top[ $regex ] = $query`), which
 * only works on declared properties. Its methods run in Rust: each reads the object's properties, does WordPress's
 * work, and writes back what it changed.
 *
 * There is no `declare(strict_types=1)`: these functions accept WordPress's loose types, as WordPress's do.
 *
 * @since 0.1.0 speedyspec-wp-rewrite-php
 */

namespace
{
    use SpeedySpec\WpRewrite\Native as N;

    foreach (
        array(
            'EP_NONE'         => 0,
            'EP_PERMALINK'    => 1,
            'EP_ATTACHMENT'   => 2,
            'EP_DATE'         => 4,
            'EP_YEAR'         => 8,
            'EP_MONTH'        => 16,
            'EP_DAY'          => 32,
            'EP_ROOT'         => 64,
            'EP_COMMENTS'     => 128,
            'EP_SEARCH'       => 256,
            'EP_CATEGORIES'   => 512,
            'EP_TAGS'         => 1024,
            'EP_AUTHORS'      => 2048,
            'EP_PAGES'        => 4096,
            'EP_ALL_ARCHIVES' => 3644,
            'EP_ALL'          => 8191,
        ) as $speedyspec_name => $speedyspec_value
    ) {
        if (! defined($speedyspec_name)) {
            define($speedyspec_name, $speedyspec_value);
        }
    }
    unset($speedyspec_name, $speedyspec_value);

    /**
     * WordPress's URL rewriting: permalink structures, rewrite tags, endpoints and rules.
     *
     * @property-read string $author_structure Unset until get_author_permastruct() caches it.
     * @property-read string $date_structure Unset until get_date_permastruct() caches it.
     * @property-read string $page_structure Unset until get_page_permastruct() caches it.
     * @property-read string $search_structure Unset until get_search_permastruct() caches it.
     * @property-read string $comment_feed_structure Unset until get_comment_feed_permastruct() caches it.
     * @property-read string $feed_structure Unset until get_feed_permastruct() caches it.
     *
     * @since 1.5.0 WordPress
     * @since 0.1.0 speedyspec-wp-rewrite-php
     */
    #[AllowDynamicProperties]
    class WP_Rewrite
    {
        /**
         * The permalink structure, from the `permalink_structure` option.
         *
         * @var string
         */
        public $permalink_structure;

        /**
         * Whether permalinks end in a slash.
         *
         * @var bool
         */
        public $use_trailing_slashes;

        /**
         * The base of author archives.
         *
         * @var string
         */
        public $author_base = 'author';

        /**
         * The author archive structure.
         *
         * @var string
         */
        public $author_structure;

        /**
         * The date archive structure.
         *
         * @var string
         */
        public $date_structure;

        /**
         * The page structure.
         *
         * @var string
         */
        public $page_structure;

        /**
         * The base of search results.
         *
         * @var string
         */
        public $search_base = 'search';

        /**
         * The search structure.
         *
         * @var string
         */
        public $search_structure;

        /**
         * The base of comment URLs.
         *
         * @var string
         */
        public $comments_base = 'comments';

        /**
         * The base of paginated URLs.
         *
         * @var string
         */
        public $pagination_base = 'page';

        /**
         * The base of comment pagination.
         *
         * @var string
         */
        public $comments_pagination_base = 'comment-page';

        /**
         * The base of feeds.
         *
         * @var string
         */
        public $feed_base = 'feed';

        /**
         * The comment feed structure.
         *
         * @var string
         */
        public $comment_feed_structure;

        /**
         * The feed structure.
         *
         * @var string
         */
        public $feed_structure;

        /**
         * The permalink structure up to its first rewrite tag.
         *
         * @var string
         */
        public $front;

        /**
         * `index.php/` under index permalinks, else empty.
         *
         * @var string
         */
        public $root = '';

        /**
         * The file every rule leads to.
         *
         * @var string
         */
        public $index = 'index.php';

        /**
         * The name of the matches array in rule queries; empty for `$1`-style references.
         *
         * @var string
         */
        public $matches = '';

        /**
         * The generated rules.
         *
         * @var string[]|string|null
         */
        public $rules;

        /**
         * Rules added with add_rewrite_rule() at the bottom.
         *
         * @var string[]
         */
        public $extra_rules = array();

        /**
         * Rules added with add_rewrite_rule() at the top, and the extra permastructs' rules.
         *
         * @var string[]
         */
        public $extra_rules_top = array();

        /**
         * Rules that lead to files other than the index file.
         *
         * @var string[]
         */
        public $non_wp_rules = array();

        /**
         * Permastructs added with add_permastruct().
         *
         * @var array[]
         */
        public $extra_permastructs = array();

        /**
         * Endpoints added with add_rewrite_endpoint(), each `array( $places, $name, $query_var )`.
         *
         * @var array[]
         */
        public $endpoints;

        /**
         * Whether .htaccess lists every rule instead of sending everything to the index file.
         *
         * @var bool
         */
        public $use_verbose_rules = false;

        /**
         * Whether page rules come before post rules.
         *
         * @var bool
         */
        public $use_verbose_page_rules = true;

        /**
         * The rewrite tags.
         *
         * @var string[]
         */
        public $rewritecode = array(
            '%year%',
            '%monthnum%',
            '%day%',
            '%hour%',
            '%minute%',
            '%second%',
            '%postname%',
            '%post_id%',
            '%author%',
            '%pagename%',
            '%search%',
        );

        /**
         * The regexes the rewrite tags stand for.
         *
         * @var string[]
         */
        public $rewritereplace = array(
            '([0-9]{4})',
            '([0-9]{1,2})',
            '([0-9]{1,2})',
            '([0-9]{1,2})',
            '([0-9]{1,2})',
            '([0-9]{1,2})',
            '([^/]+)',
            '([0-9]+)',
            '([^/]+)',
            '([^/]+?)',
            '(.+)',
        );

        /**
         * The query variables the rewrite tags set.
         *
         * @var string[]
         */
        public $queryreplace = array(
            'year=',
            'monthnum=',
            'day=',
            'hour=',
            'minute=',
            'second=',
            'name=',
            'p=',
            'author_name=',
            'pagename=',
            's=',
        );

        /**
         * The feed names.
         *
         * @var string[]
         */
        public $feeds = array('feed', 'rdf', 'rss', 'rss2', 'atom');

        public function __construct()
        {
            $this->init();
        }

        /**
         * Hands the Rust port the object's properties; unset ones are left out, and so are the rules, which only
         * rule generation reads (through __speedyspec_rules()).
         *
         * @internal
         */
        public function __speedyspec_vars()
        {
            $vars = get_object_vars($this);
            unset($vars['rules']);
            return $vars;
        }

        /**
         * Hands the Rust port the permastructs and endpoints as PHP holds them, so a write keeps their types.
         *
         * @internal
         */
        public function __speedyspec_shapes()
        {
            return array(
                isset($this->extra_permastructs) ? $this->extra_permastructs : array(),
                isset($this->endpoints) ? $this->endpoints : array(),
            );
        }

        /**
         * Hands the Rust port the rules alone.
         *
         * @internal
         */
        public function __speedyspec_rules()
        {
            return $this->rules;
        }

        /**
         * Writes back what a method changed, and unsets what it unset.
         *
         * @internal
         */
        public function __speedyspec_assign($vars, $unset)
        {
            foreach ($vars as $name => $value) {
                $this->$name = $value;
            }
            foreach ($unset as $name) {
                unset($this->$name);
            }
        }

        /**
         * As WordPress has it: a read this frequent and this simple stays in PHP.
         */
        public function using_permalinks()
        {
            return ! empty($this->permalink_structure);
        }

        /**
         * As WordPress has it: a read this frequent and this simple stays in PHP.
         */
        public function using_index_permalinks()
        {
            if (empty($this->permalink_structure)) {
                return false;
            }

            return preg_match('#^/*' . $this->index . '#', $this->permalink_structure);
        }

        /**
         * As WordPress has it: a read this frequent and this simple stays in PHP.
         */
        public function using_mod_rewrite_permalinks()
        {
            return $this->using_permalinks() && ! $this->using_index_permalinks();
        }

        /**
         * As WordPress has it: a read this frequent and this simple stays in PHP.
         */
        public function preg_index($number)
        {
            $match_prefix = '$';
            $match_suffix = '';

            if (! empty($this->matches)) {
                $match_prefix = '$' . $this->matches . '[';
                $match_suffix = ']';
            }

            return "$match_prefix$number$match_suffix";
        }

        /**
         * Retrieves every page's URI and every page attachment's URI, keyed by URI, as WordPress does.
         *
         * Kept in PHP: it reads posts, which are not ported yet.
         *
         * @return array
         *   Two arrays: page URIs to IDs, and page attachment URIs to IDs.
         */
        public function page_uri_index()
        {
            global $wpdb;

            $pages = $wpdb->get_results(
                "SELECT ID, post_name, post_parent FROM $wpdb->posts WHERE post_type = 'page' AND post_status != 'auto-draft'"
            );
            $posts = get_page_hierarchy($pages);
            if (! $posts) {
                return array(array(), array());
            }
            $posts                = array_reverse($posts, true);
            $page_uris            = array();
            $page_attachment_uris = array();
            foreach ($posts as $id => $post) {
                $uri         = get_page_uri($id);
                $attachments = $wpdb->get_results(
                    $wpdb->prepare(
                        "SELECT ID, post_name, post_parent FROM $wpdb->posts WHERE post_type = 'attachment' AND post_parent = %d",
                        $id
                    )
                );
                if (! empty($attachments)) {
                    foreach ($attachments as $attachment) {
                        $attach_uri                          = get_page_uri($attachment->ID);
                        $page_attachment_uris[ $attach_uri ] = $attachment->ID;
                    }
                }
                $page_uris[ $uri ] = $id;
            }
            return array($page_uris, $page_attachment_uris);
        }

        public function page_rewrite_rules()
        {
            return N\call($this, __FUNCTION__, func_get_args());
        }

        public function get_date_permastruct()
        {
            return N\call($this, __FUNCTION__, func_get_args());
        }

        public function get_year_permastruct()
        {
            return N\call($this, __FUNCTION__, func_get_args());
        }

        public function get_month_permastruct()
        {
            return N\call($this, __FUNCTION__, func_get_args());
        }

        public function get_day_permastruct()
        {
            return N\call($this, __FUNCTION__, func_get_args());
        }

        public function get_category_permastruct()
        {
            return N\call($this, __FUNCTION__, func_get_args());
        }

        public function get_tag_permastruct()
        {
            return N\call($this, __FUNCTION__, func_get_args());
        }

        public function get_extra_permastruct($name)
        {
            return N\call($this, __FUNCTION__, func_get_args());
        }

        public function get_author_permastruct()
        {
            return N\call($this, __FUNCTION__, func_get_args());
        }

        public function get_search_permastruct()
        {
            return N\call($this, __FUNCTION__, func_get_args());
        }

        public function get_page_permastruct()
        {
            return N\call($this, __FUNCTION__, func_get_args());
        }

        public function get_feed_permastruct()
        {
            return N\call($this, __FUNCTION__, func_get_args());
        }

        public function get_comment_feed_permastruct()
        {
            return N\call($this, __FUNCTION__, func_get_args());
        }

        public function add_rewrite_tag($tag, $regex, $query)
        {
            return N\call($this, __FUNCTION__, func_get_args());
        }

        public function remove_rewrite_tag($tag)
        {
            return N\call($this, __FUNCTION__, func_get_args());
        }

        public function generate_rewrite_rules(
            $permalink_structure,
            $ep_mask = EP_NONE,
            $paged = true,
            $feed = true,
            $forcomments = false,
            $walk_dirs = true,
            $endpoints = true
        ) {
            return N\call($this, __FUNCTION__, func_get_args());
        }

        public function generate_rewrite_rule($permalink_structure, $walk_dirs = false)
        {
            return N\call($this, __FUNCTION__, func_get_args());
        }

        public function rewrite_rules()
        {
            return N\call($this, __FUNCTION__, func_get_args());
        }

        public function wp_rewrite_rules()
        {
            return N\call($this, __FUNCTION__, func_get_args());
        }

        public function mod_rewrite_rules()
        {
            return N\call($this, __FUNCTION__, func_get_args());
        }

        public function iis7_url_rewrite_rules($add_parent_tags = false)
        {
            return N\call($this, __FUNCTION__, func_get_args());
        }

        public function add_rule($regex, $query, $after = 'bottom')
        {
            return N\call($this, __FUNCTION__, func_get_args());
        }

        public function add_external_rule($regex, $query)
        {
            return N\call($this, __FUNCTION__, func_get_args());
        }

        public function add_endpoint($name, $places, $query_var = true)
        {
            return N\call($this, __FUNCTION__, func_get_args());
        }

        public function add_permastruct($name, $struct, $args = array())
        {
            return N\call($this, __FUNCTION__, func_get_args());
        }

        public function remove_permastruct($name)
        {
            return N\call($this, __FUNCTION__, func_get_args());
        }

        public function flush_rules($hard = true)
        {
            return N\call($this, __FUNCTION__, func_get_args());
        }

        public function init()
        {
            return N\call($this, __FUNCTION__, func_get_args());
        }

        public function set_permalink_structure($permalink_structure)
        {
            return N\call($this, __FUNCTION__, func_get_args());
        }

        public function set_category_base($category_base)
        {
            return N\call($this, __FUNCTION__, func_get_args());
        }

        public function set_tag_base($tag_base)
        {
            return N\call($this, __FUNCTION__, func_get_args());
        }
    }

    /**
     * Substitutes `$matches[N]` references in a rule's query with URL-encoded captures.
     *
     * @since 2.9.0 WordPress
     * @since 0.1.0 speedyspec-wp-rewrite-php
     */
    #[AllowDynamicProperties]
    class WP_MatchesMapRegex
    {
        /**
         * The substituted subject.
         *
         * @var string
         */
        public $output;

        /**
         * The reference pattern WordPress matches.
         *
         * @var string
         */
        public $_pattern = '(\$matches\[[1-9]+[0-9]*\])';

        private $_subject;

        private $_matches;

        public function __construct($subject, $matches)
        {
            $this->_subject = $subject;
            $this->_matches = $matches;
            $this->output   = N\matches_map((string) $subject, (array) $matches);
        }

        public static function apply($subject, $matches)
        {
            return N\matches_map((string) $subject, (array) $matches);
        }

        /**
         * Answers one reference, as WordPress's preg_replace_callback() callback does.
         *
         * @param array $matches
         *   The reference match.
         * @return string
         *   The URL-encoded capture, or an empty string.
         */
        public function callback($matches)
        {
            $index = (int) substr($matches[0], 9, -1);
            return (isset($this->_matches[ $index ]) ? urlencode($this->_matches[ $index ]) : '');
        }
    }

    function add_rewrite_rule($regex, $query, $after = 'bottom')
    {
        global $wp_rewrite;

        $wp_rewrite->add_rule($regex, $query, $after);
    }

    function add_rewrite_tag($tag, $regex, $query = '')
    {
        if (strlen($tag) < 3 || '%' !== $tag[0] || '%' !== $tag[ strlen($tag) - 1 ]) {
            return;
        }

        global $wp_rewrite, $wp;

        if (empty($query)) {
            $qv = trim($tag, '%');
            $wp->add_query_var($qv);
            $query = $qv . '=';
        }

        $wp_rewrite->add_rewrite_tag($tag, $regex, $query);
    }

    function remove_rewrite_tag($tag)
    {
        global $wp_rewrite;

        $wp_rewrite->remove_rewrite_tag($tag);
    }

    function add_permastruct($name, $struct, $args = array())
    {
        global $wp_rewrite;

        if (! is_array($args)) {
            $args = array('with_front' => $args);
        }

        if (func_num_args() === 4) {
            $args['ep_mask'] = func_get_arg(3);
        }

        $wp_rewrite->add_permastruct($name, $struct, $args);
    }

    function remove_permastruct($name)
    {
        global $wp_rewrite;

        $wp_rewrite->remove_permastruct($name);
    }

    function add_feed($feedname, $callback)
    {
        global $wp_rewrite;

        if (! in_array($feedname, $wp_rewrite->feeds, true)) {
            $wp_rewrite->feeds[] = $feedname;
        }

        $hook = 'do_feed_' . $feedname;

        remove_action($hook, $hook);
        add_action($hook, $callback, 10, 2);

        return $hook;
    }

    function flush_rewrite_rules($hard = true)
    {
        global $wp_rewrite;

        if (is_callable(array($wp_rewrite, 'flush_rules'))) {
            $wp_rewrite->flush_rules($hard);
        }
    }

    function add_rewrite_endpoint($name, $places, $query_var = true)
    {
        global $wp_rewrite;

        $wp_rewrite->add_endpoint($name, $places, $query_var);
    }

    /**
     * Runs WordPress's own rule loop from WP::parse_request, from rule `$from` on.
     *
     *  speedyspec_rewrite_match() hands it the rules from the first one no Rust regex engine compiles, and
     *  WP::parse_request calls it from rule 0 when the rules cannot reach Rust at all (a rule key that is not UTF-8).
     *  The requested-file anchor is replayed over the rules before `$from`, as the loop would have set it.
     *
     * @return array
     *   The matched rule, query and query variables, each null when nothing matched.
     */
    function speedyspec_rewrite_match_php($rewrite, $requested_path, $requested_file, $from = 0)
    {
        global $wp_rewrite;

        $request_match = $requested_path;
        if (empty($request_match)) {
            if (0 === $from && isset($rewrite['$'])) {
                $query = addslashes(WP_MatchesMapRegex::apply(preg_replace('!^.+\?!', '', $rewrite['$']), array('')));
                parse_str($query, $perma_query_vars);
                return array('matched_rule' => '$', 'matched_query' => $query, 'perma_query_vars' => $perma_query_vars);
            }
            return array('matched_rule' => null, 'matched_query' => null, 'perma_query_vars' => null);
        }
        $i = -1;
        foreach ((array) $rewrite as $match => $query) {
            ++$i;
            if (! empty($requested_file) && str_starts_with($match, $requested_file) && $requested_file !== $requested_path) {
                $request_match = $requested_file . '/' . $requested_path;
            }
            if ($i < $from) {
                continue;
            }
            if (preg_match("#^$match#", $request_match, $matches) || preg_match("#^$match#", urldecode($request_match), $matches)) {
                if (! empty($wp_rewrite->use_verbose_page_rules) && preg_match('/pagename=\$matches\[([0-9]+)\]/', $query, $varmatch)) {
                    $page = get_page_by_path($matches[ $varmatch[1] ]);
                    if (! $page) {
                        continue;
                    }
                    $post_status_obj = get_post_status_object($page->post_status);
                    if (! $post_status_obj->public && ! $post_status_obj->protected && ! $post_status_obj->private && $post_status_obj->exclude_from_search) {
                        continue;
                    }
                }
                $query = preg_replace('!^.+\?!', '', $query);
                $query = addslashes(WP_MatchesMapRegex::apply($query, $matches));
                parse_str($query, $perma_query_vars);
                return array('matched_rule' => $match, 'matched_query' => $query, 'perma_query_vars' => $perma_query_vars);
            }
        }
        return array('matched_rule' => null, 'matched_query' => null, 'perma_query_vars' => null);
    }

    function _wp_filter_taxonomy_base($base)
    {
        if (! empty($base)) {
            $base = preg_replace('|^/index\.php/|', '', $base);
            $base = trim($base, '/');
        }
        return $base;
    }
}
