use crate::TagName;

/// PHP 8.5's `preg_quote()` with no delimiter.
pub fn preg_quote(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() * 2);
    for &b in s {
        match b {
            b'.' | b'\\' | b'+' | b'*' | b'?' | b'[' | b'^' | b']' | b'$' | b'(' | b')' | b'{' | b'}' | b'='
            | b'!' | b'<' | b'>' | b'|' | b':' | b'-' | b'#' => {
                out.push(b'\\');
                out.push(b);
            }
            0 => out.extend_from_slice(b"\\000"),
            _ => out.push(b),
        }
    }
    out
}

/// `implode( '|', array_map( 'preg_quote', $tagnames ) )`, which `get_shortcode_regex()` and `wptexturize`
/// both build.
pub fn shortcode_tag_alternation(names: &[TagName]) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, n) in names.iter().enumerate() {
        if i > 0 {
            out.push(b'|');
        }
        out.extend_from_slice(&preg_quote(n.as_bytes()));
    }
    out
}

/// `get_shortcode_regex( $tagnames )`, without delimiters, exactly as WordPress builds it.
pub fn get_shortcode_regex(names: &[TagName]) -> Vec<u8> {
    let mut out = br"\[(\[?)(".to_vec();
    out.extend_from_slice(&shortcode_tag_alternation(names));
    out.extend_from_slice(
        br")(?![\w-])([^\]\/]*(?:\/(?!\])[^\]\/]*)*?)(?:(\/)\]|\](?:([^\[]*+(?:\[(?!\/\2\])[^\[]*+)*+)\[\/\2\])?)(\]?)",
    );
    out
}

/// `get_shortcode_atts_regex()`, with its delimiters, exactly as WordPress returns it.
pub fn get_shortcode_atts_regex() -> &'static [u8] {
    br#"/([\w-]+)\s*=\s*"([^"]*)"(?:\s|$)|([\w-]+)\s*=\s*'([^']*)'(?:\s|$)|([\w-]+)\s*=\s*([^\s'"]+)(?:\s|$)|"([^"]*)"(?:\s|$)|'([^']*)'(?:\s|$)|(\S+)(?:\s|$)/"#
}
