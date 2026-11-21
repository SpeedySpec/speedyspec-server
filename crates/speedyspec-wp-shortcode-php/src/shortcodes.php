// Not strict: WordPress's shortcodes.php coerces arguments (str_contains( 42, '[' ) is allowed), and so must this.

namespace SpeedySpec\WpShortcode\Support
{
    /**
     * Writes one entry of the shortcode registry, as `add_shortcode()` does.
     *
     *  The write happens in PHP so a copy of the array someone saved keeps its own entries.
     */
    function set_tag($tag, $callback)
    {
        $GLOBALS['shortcode_tags'][$tag] = $callback;
    }

    /**
     * Removes one entry of the shortcode registry, as `remove_shortcode()` does.
     */
    function unset_tag($tag)
    {
        unset($GLOBALS['shortcode_tags'][$tag]);
    }

    /**
     * Empties the shortcode registry, as `remove_all_shortcodes()` does.
     */
    function clear_tags()
    {
        $GLOBALS['shortcode_tags'] = array();
    }

    /**
     * Reads a key that is not there, so PHP raises its own "Undefined array key" warning where WordPress would.
     */
    function read_missing_key($key)
    {
        $none = array();
        return $none[$key];
    }
}

namespace
{
    use SpeedySpec\WpShortcode\Native as N;

    $GLOBALS['shortcode_tags'] = array();

    /**
     * Adds a new shortcode.
     */
    function add_shortcode($tag, $callback)
    {
        N\add_shortcode($tag, $callback);
    }

    /**
     * Removes hook for shortcode.
     */
    function remove_shortcode($tag)
    {
        N\remove_shortcode($tag);
    }

    /**
     * Clears all shortcodes.
     */
    function remove_all_shortcodes()
    {
        N\remove_all_shortcodes();
    }

    /**
     * Determines whether a registered shortcode exists named $tag.
     */
    function shortcode_exists($tag)
    {
        return N\shortcode_exists($tag);
    }

    /**
     * Determines whether the passed content contains the specified shortcode.
     */
    function has_shortcode($content, $tag)
    {
        if (!str_contains($content, '[')) {
            return false;
        }
        return N\has_shortcode($content, $tag);
    }

    /**
     * Returns a list of registered shortcode names found in the given content.
     */
    function get_shortcode_tags_in_content($content)
    {
        if (!str_contains($content, '[')) {
            return array();
        }
        return N\get_shortcode_tags_in_content($content);
    }

    /**
     * Searches content for shortcodes and filters shortcodes through their hooks.
     */
    function apply_shortcodes($content, $ignore_html = false)
    {
        return do_shortcode($content, $ignore_html);
    }

    /**
     * Searches content for shortcodes and filters shortcodes through their hooks.
     */
    function do_shortcode($content, $ignore_html = false)
    {
        if (!str_contains($content, '[')) {
            return $content;
        }
        return N\do_shortcode($content, $ignore_html);
    }

    /**
     * Filter the `wp_get_attachment_image_context` hook during shortcode rendering.
     */
    function _filter_do_shortcode_context()
    {
        return 'do_shortcode';
    }

    /**
     * Retrieves the shortcode regular expression for searching.
     */
    function get_shortcode_regex($tagnames = null)
    {
        return N\get_shortcode_regex($tagnames);
    }

    /**
     * Regular Expression callable for do_shortcode() for calling shortcode hook.
     */
    function do_shortcode_tag($m)
    {
        return N\do_shortcode_tag($m);
    }

    /**
     * Searches only inside HTML elements for shortcodes and process them.
     */
    function do_shortcodes_in_html_tags($content, $ignore_html, $tagnames)
    {
        return N\do_shortcodes_in_html_tags($content, $ignore_html, $tagnames);
    }

    /**
     * Removes placeholders added by do_shortcodes_in_html_tags().
     */
    function unescape_invalid_shortcodes($content)
    {
        return N\unescape_invalid_shortcodes($content);
    }

    /**
     * Retrieves the shortcode attributes regex.
     */
    function get_shortcode_atts_regex()
    {
        return '/([\w-]+)\s*=\s*"([^"]*)"(?:\s|$)|([\w-]+)\s*=\s*\'([^\']*)\'(?:\s|$)|([\w-]+)\s*=\s*([^\s\'"]+)(?:\s|$)|"([^"]*)"(?:\s|$)|\'([^\']*)\'(?:\s|$)|(\S+)(?:\s|$)/';
    }

    /**
     * Retrieves all attributes from the shortcodes tag.
     */
    function shortcode_parse_atts($text)
    {
        return N\shortcode_parse_atts($text);
    }

    /**
     * Combines user attributes with known attributes and fill in defaults when needed.
     */
    function shortcode_atts($pairs, $atts, $shortcode = '')
    {
        return N\shortcode_atts($pairs, $atts, $shortcode);
    }

    /**
     * Removes all shortcode tags from the given content.
     */
    function strip_shortcodes($content)
    {
        if (!str_contains($content, '[')) {
            return $content;
        }
        return N\strip_shortcodes($content);
    }

    /**
     * Strips a shortcode tag based on RegEx matches against post content.
     */
    function strip_shortcode_tag($m)
    {
        return N\strip_shortcode_tag($m);
    }
}
