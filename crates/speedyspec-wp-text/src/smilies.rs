//! Smileys: `smilies_init()`'s search pattern, `translate_smiley()` and `convert_smilies()`.

use speedyspec_wp_shortcode_contracts::preg_quote;

/// `preg_quote( $text, '/' )`.
fn quote(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() * 2);
    for chunk in preg_quote(text).split_inclusive(|&b| b == b'/') {
        match chunk.split_last() {
            Some((b'/', head)) => {
                out.extend_from_slice(head);
                out.extend_from_slice(b"\\/");
            }
            _ => out.extend_from_slice(chunk),
        }
    }
    out
}

/// The search pattern `smilies_init()` builds into `$wp_smiliessearch`.
///
/// @param table
///     `$wpsmiliestrans` after its `smilies` filter: `(smiley, image)` pairs.
/// @param spaces
///     `wp_spaces_regexp()`.
/// @return
///     The pattern, or `None` when the table is empty (WordPress leaves `$wp_smiliessearch` unset).
pub fn smilies_search_pattern(table: &[(Vec<u8>, Vec<u8>)], spaces: &[u8]) -> Option<Vec<u8>> {
    if table.is_empty() {
        return None;
    }
    // krsort(): keys in descending byte order.
    let mut smileys: Vec<&[u8]> = table.iter().map(|(s, _)| s.as_slice()).collect();
    smileys.sort_unstable_by(|a, b| b.cmp(a));
    let mut out = [b"/(?<=", spaces, b"|^)"].concat();
    let mut subchar: Option<&[u8]> = None;
    for smiley in smileys {
        let (first, rest) = smiley.split_at(smiley.len().min(1));
        if subchar != Some(first) {
            if subchar.is_some() {
                out.extend_from_slice(&[b")(?=", spaces, b"|$)"].concat());
                out.extend_from_slice(&[b"|(?<=", spaces, b"|^)"].concat());
            }
            subchar = Some(first);
            out.extend_from_slice(&quote(first));
            out.extend_from_slice(b"(?:");
        } else {
            out.push(b'|');
        }
        out.extend_from_slice(&quote(rest));
    }
    out.extend_from_slice(&[b")(?=", spaces, b"|$)/m"].concat());
    Some(out)
}
