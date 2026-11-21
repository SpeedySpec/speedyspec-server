// Evaluated without an opening tag. No strict_types: WordPress's own pluggable.php is not strict, so arguments coerce as they do there.

namespace SpeedySpec\WpTextDiff
{
    /**
     * Whether the native table renders `$args` exactly as WordPress would.
     *
     *  The native path covers string titles and every `show_split_view`. WordPress's renderer also looks at every
     *  other argument: one named after its `_`-prefixed properties (`leading_context_lines`, `trailing_context_lines`,
     *  `diff_threshold`) changes the output, and any other triggers a deprecation notice; and it prints titles of any
     *  type by string interpolation. Those calls go to {@link legacy_wp_text_diff()}, so their behaviour, warnings
     *  included, stays WordPress's.
     *
     * @param array $args
     *   The arguments after `wp_parse_args()`.
     * @return bool
     *   True when the native function renders them.
     */
    function native_args(array $args): bool
    {
        return is_string($args['title'])
            && is_string($args['title_left'])
            && is_string($args['title_right'])
            && array() === array_diff_key($args, array_flip(array('title', 'title_left', 'title_right', 'show_split_view')));
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
         *  Native: the diff, the rows and the table are rendered in Rust, byte for byte as WordPress renders them.
         *  The row labels are translated once per call rather than once per row.
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
         */
        function wp_text_diff($left_string, $right_string, $args = null)
        {
            $defaults = array(
                'title'           => '',
                'title_left'      => '',
                'title_right'     => '',
                'show_split_view' => true,
            );
            $args = wp_parse_args($args, $defaults);

            // As WordPress does: the classes are defined after the first call, for code that relies on it.
            if (!class_exists('WP_Text_Diff_Renderer_Table', false))
            {
                require ABSPATH . WPINC . '/wp-diff.php';
            }

            if (!\SpeedySpec\WpTextDiff\native_args($args))
            {
                return \SpeedySpec\WpTextDiff\legacy_wp_text_diff($left_string, $right_string, $args);
            }

            $left_string  = normalize_whitespace($left_string);
            $right_string = normalize_whitespace($right_string);
            if ($left_string === $right_string)
            {
                return '';
            }

            return \SpeedySpec\WpTextDiff\Native\wp_text_diff(
                $left_string,
                $right_string,
                $args['title'],
                $args['title_left'],
                $args['title_right'],
                !empty($args['show_split_view']),
                /* translators: Hidden accessibility text. */
                __('Added:'),
                /* translators: Hidden accessibility text. */
                __('Deleted:'),
                /* translators: Hidden accessibility text. */
                __('Unchanged:')
            );
        }
    }
}
