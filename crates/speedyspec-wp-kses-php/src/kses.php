/**
 * The pinned WordPress release's kses functions, declared over SpeedySpec's native kses (generated from the
 * release's own signatures; see release.rs for the list). Evaluated as code: no opening tag.
 */

namespace
{
    use SpeedySpec\WpKses\Native as N;

    function wp_kses($content, $allowed_html, $allowed_protocols = array())
    {
        return N\wp_kses(...func_get_args());
    }

    function wp_kses_one_attr($attr, $element)
    {
        return N\wp_kses_one_attr(...func_get_args());
    }

    function wp_kses_allowed_html($context = '')
    {
        return N\wp_kses_allowed_html(...func_get_args());
    }

    function _wp_kses_allow_note_mention_span($allowed, $context): array
    {
        return N\_wp_kses_allow_note_mention_span(...func_get_args());
    }

    function wp_kses_hook($content, $allowed_html, $allowed_protocols)
    {
        return N\wp_kses_hook(...func_get_args());
    }

    function wp_kses_version()
    {
        return N\wp_kses_version(...func_get_args());
    }

    /**
     * Sets the globals `_wp_kses_split_callback()` reads, as WordPress does.
     */
    function wp_kses_split($content, $allowed_html, $allowed_protocols)
    {
        global $pass_allowed_html, $pass_allowed_protocols;
        $pass_allowed_html      = $allowed_html;
        $pass_allowed_protocols = $allowed_protocols;
        return N\wp_kses_split(...func_get_args());
    }

    function wp_kses_uri_attributes()
    {
        return N\wp_kses_uri_attributes(...func_get_args());
    }

    function _wp_kses_split_callback($matches)
    {
        return N\_wp_kses_split_callback(...func_get_args());
    }

    function wp_kses_split2($content, $allowed_html, $allowed_protocols)
    {
        return N\wp_kses_split2(...func_get_args());
    }

    function wp_kses_attr($element, $attr, $allowed_html, $allowed_protocols)
    {
        return N\wp_kses_attr(...func_get_args());
    }

    /**
     * Native returns `[ok, name, value, whole]`; the references are written here.
     */
    function wp_kses_attr_check(&$name, &$value, &$whole, $vless, $element, $allowed_html)
    {
        $checked = N\wp_kses_attr_check($name, $value, $whole, $vless, $element, $allowed_html);
        $name    = $checked[1];
        $value   = $checked[2];
        $whole   = $checked[3];
        return $checked[0];
    }

    function wp_kses_hair($attr, $allowed_protocols)
    {
        return N\wp_kses_hair(...func_get_args());
    }

    function wp_kses_attr_parse($element)
    {
        return N\wp_kses_attr_parse(...func_get_args());
    }

    function wp_kses_hair_parse($attr)
    {
        return N\wp_kses_hair_parse(...func_get_args());
    }

    function wp_kses_check_attr_val($value, $vless, $checkname, $checkvalue)
    {
        return N\wp_kses_check_attr_val(...func_get_args());
    }

    function wp_kses_bad_protocol($content, $allowed_protocols)
    {
        return N\wp_kses_bad_protocol(...func_get_args());
    }

    function wp_kses_no_null($content, $options = null)
    {
        return N\wp_kses_no_null(...func_get_args());
    }

    function wp_kses_stripslashes($content)
    {
        return N\wp_kses_stripslashes(...func_get_args());
    }

    function wp_kses_array_lc($inarray)
    {
        return N\wp_kses_array_lc(...func_get_args());
    }

    function wp_kses_bad_protocol_once($content, $allowed_protocols, $count = 1)
    {
        return N\wp_kses_bad_protocol_once(...func_get_args());
    }

    function wp_kses_bad_protocol_once2($scheme, $allowed_protocols)
    {
        return N\wp_kses_bad_protocol_once2(...func_get_args());
    }

    function wp_kses_normalize_entities($content, $context = 'html')
    {
        return N\wp_kses_normalize_entities(...func_get_args());
    }

    function wp_kses_named_entities($matches)
    {
        return N\wp_kses_named_entities(...func_get_args());
    }

    function wp_kses_xml_named_entities($matches)
    {
        return N\wp_kses_xml_named_entities(...func_get_args());
    }

    function wp_kses_normalize_entities2($matches)
    {
        return N\wp_kses_normalize_entities2(...func_get_args());
    }

    function wp_kses_normalize_entities3($matches)
    {
        return N\wp_kses_normalize_entities3(...func_get_args());
    }

    function valid_unicode($i)
    {
        return N\valid_unicode(...func_get_args());
    }

    function wp_kses_decode_entities($content)
    {
        return N\wp_kses_decode_entities(...func_get_args());
    }

    function _wp_kses_decode_entities_chr($matches)
    {
        return N\_wp_kses_decode_entities_chr(...func_get_args());
    }

    function _wp_kses_decode_entities_chr_hexdec($matches)
    {
        return N\_wp_kses_decode_entities_chr_hexdec(...func_get_args());
    }

    function wp_filter_kses($data)
    {
        return N\wp_filter_kses(...func_get_args());
    }

    function wp_kses_data($data)
    {
        return N\wp_kses_data(...func_get_args());
    }

    function wp_filter_post_kses($data)
    {
        return N\wp_filter_post_kses(...func_get_args());
    }

    function wp_kses_post($data)
    {
        return N\wp_kses_post(...func_get_args());
    }

    /**
     * WordPress's own body: only PHP can walk an object's properties. Each leaf goes to the native wp_kses_post().
     */
    function wp_kses_post_deep($data)
    {
        return map_deep($data, 'wp_kses_post');
    }

    function wp_filter_nohtml_kses($data)
    {
        return N\wp_filter_nohtml_kses(...func_get_args());
    }

    function kses_init_filters()
    {
        return N\kses_init_filters(...func_get_args());
    }

    function kses_remove_filters()
    {
        return N\kses_remove_filters(...func_get_args());
    }

    function kses_init()
    {
        return N\kses_init(...func_get_args());
    }

    /**
     * The deprecation notice is raised here, as WordPress raises it.
     */
    function safecss_filter_attr($css, $deprecated = '')
    {
        if (! empty($deprecated))
        {
            _deprecated_argument(__FUNCTION__, '2.8.1'); // Never implemented.
        }
        return N\safecss_filter_attr($css);
    }

    function _wp_add_global_attributes($value)
    {
        return N\_wp_add_global_attributes(...func_get_args());
    }

    function _wp_kses_allow_pdf_objects($url)
    {
        return N\_wp_kses_allow_pdf_objects(...func_get_args());
    }

    function _wp_kses_restricts_attribute_values($allowed_attributes): bool
    {
        return N\_wp_kses_restricts_attribute_values(...func_get_args());
    }

    function _wp_kses_directive_binds_value_restricted_attribute(string $directive_name, $allowed_attributes): bool
    {
        return N\_wp_kses_directive_binds_value_restricted_attribute(...func_get_args());
    }

    function wp_allowed_protocols()
    {
        return N\wp_allowed_protocols(...func_get_args());
    }

}
