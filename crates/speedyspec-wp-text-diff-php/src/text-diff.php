// Evaluated without an opening tag. No strict_types: WordPress's own pluggable.php is not strict, so arguments coerce as they do there.

namespace SpeedySpec\WpTextDiff
{
    /**
     * Parses `wp_text_diff()`'s arguments as WordPress does.
     *
     * @param string|array|null $args
     *   The arguments as the caller passed them.
     * @return array
     *   The arguments after `wp_parse_args()`, over WordPress's defaults.
     */
    function parse_args($args): array
    {
        $defaults = array(
            'title'           => '',
            'title_left'      => '',
            'title_right'     => '',
            'show_split_view' => true,
        );
        return KeptArgs::$args = wp_parse_args($args, $defaults);
    }

    /**
     * The arguments {@link parse_args()} last returned, for a call whose arguments Rust cannot read.
     *
     * @internal
     */
    final class KeptArgs
    {
        /**
         * The arguments after `wp_parse_args()`.
         *
         * @var array<array-key, mixed>
         */
        public static array $args = array();
    }

    /**
     * Renders with WordPress's own classes and the arguments {@link parse_args()} last returned, for a call whose
     * parsed arguments cannot reach Rust (a key that isn't UTF-8, a recursive array).
     *
     * @param mixed $left_string
     *   The "old" (left) version of the string.
     * @param mixed $right_string
     *   The "new" (right) version of the string.
     * @return string
     *   An empty string if the strings are equivalent, otherwise the HTML table.
     */
    function legacy_with_kept_args($left_string, $right_string)
    {
        $args           = KeptArgs::$args;
        KeptArgs::$args = array();
        return legacy_wp_text_diff($left_string, $right_string, $args);
    }

    /**
     * Loads WordPress's diff classes, as `wp_text_diff()` does on every call, for code that relies on them.
     *
     * @return void
     */
    function load_renderer(): void
    {
        if (!class_exists('WP_Text_Diff_Renderer_Table', false))
        {
            require ABSPATH . WPINC . '/wp-diff.php';
        }
    }

    /**
     * The hidden row labels, translated once per call rather than once per row.
     *
     * @return string[]
     *   The labels of added, deleted and unchanged cells, in that order.
     *
     * @phpstan-return array{0: string, 1: string, 2: string}
     */
    function row_labels(): array
    {
        return array(
            /* translators: Hidden accessibility text. */
            __('Added:'),
            /* translators: Hidden accessibility text. */
            __('Deleted:'),
            /* translators: Hidden accessibility text. */
            __('Unchanged:'),
        );
    }

    /**
     * Renders a diff with WordPress's own PHP classes, for the arguments the native table does not cover.
     *
     *  The body is WordPress 7.1's `wp_text_diff()` after `wp_parse_args()`.
     *
     * @param mixed $left_string
     *   The "old" (left) version of the string.
     * @param mixed $right_string
     *   The "new" (right) version of the string.
     * @param array $args
     *   The arguments after `wp_parse_args()`.
     * @return string
     *   An empty string if the strings are equivalent, otherwise the HTML table.
     */
    function legacy_wp_text_diff($left_string, $right_string, array $args)
    {
        if (!class_exists('WP_Text_Diff_Renderer_Table', false))
        {
            require ABSPATH . WPINC . '/wp-diff.php';
        }

        $left_string  = normalize_whitespace($left_string);
        $right_string = normalize_whitespace($right_string);

        $left_lines  = explode("\n", $left_string);
        $right_lines = explode("\n", $right_string);
        $text_diff   = new \Text_Diff($left_lines, $right_lines);
        $renderer    = new \WP_Text_Diff_Renderer_Table($args);
        $diff        = $renderer->render($text_diff);

        if (!$diff)
        {
            return '';
        }

        $is_split_view       = !empty($args['show_split_view']);
        $is_split_view_class = $is_split_view ? ' is-split-view' : '';

        $r = "<table class='diff$is_split_view_class'>\n";

        if ($args['title'])
        {
            $r .= "<caption class='diff-title'>$args[title]</caption>\n";
        }

        if ($args['title_left'] || $args['title_right'])
        {
            $r .= '<thead>';
        }

        if ($args['title_left'] || $args['title_right'])
        {
            $th_or_td_left  = empty($args['title_left']) ? 'td' : 'th';
            $th_or_td_right = empty($args['title_right']) ? 'td' : 'th';

            $r .= "<tr class='diff-sub-title'>\n";
            $r .= "\t<$th_or_td_left>$args[title_left]</$th_or_td_left>\n";
            if ($is_split_view)
            {
                $r .= "\t<$th_or_td_right>$args[title_right]</$th_or_td_right>\n";
            }
            $r .= "</tr>\n";
        }

        if ($args['title_left'] || $args['title_right'])
        {
            $r .= "</thead>\n";
        }

        $r .= "<tbody>\n$diff\n</tbody>\n";
        $r .= '</table>';

        return $r;
    }
}

namespace
{
    if (!function_exists('wp_text_diff'))
    {
        /**
         * Displays a human readable HTML representation of the difference between two strings.
         *
         *  The implementation is the container's `SpeedySpec\WpTextDiff\TextDiffer`, which PHP can rebind; it is
         *  given the arguments untouched. WordPress's implementation normalizes the whitespace and renders the diff,
         *  the rows and the table in Rust, byte for byte as WordPress renders them, and translates the row labels once
         *  per call rather than once per row.
         *
         * @param string       $left_string
         *   The "old" (left) version of the string.
         * @param string       $right_string
         *   The "new" (right) version of the string.
         * @param string|array $args
         *   `title`, `title_left`, `title_right` and `show_split_view`, as in WordPress.
         * @return string
         *   An empty string if the strings are equivalent, otherwise the HTML table.
         *
         * @since 2.6.0 WordPress
         * @since 0.1.0 speedyspec-wp-text-diff-php
         * @since 0.2.0 speedyspec-wp-text-diff-php Resolves the container's `TextDiffer` on every call.
         */
        function wp_text_diff($left_string, $right_string, $args = null)
        {
            return \SpeedySpec\WpTextDiff\Native\wp_text_diff($left_string, $right_string, $args);
        }
    }
}
