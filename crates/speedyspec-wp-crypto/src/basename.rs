//! `wp_basename()`, for the file name in signature errors.

/// The last path component, as WordPress's `wp_basename()` with no suffix: both `/` and `\` separate components
/// (WordPress swaps them after `urlencode()`), trailing separators are ignored, and the bytes of the name are kept as
/// they are, multibyte or not.
pub fn wp_basename(path: &[u8]) -> Vec<u8> {
    let is_separator = |b: &u8| *b == b'/' || *b == b'\\';
    let end = path
        .iter()
        .rposition(|b| !is_separator(b))
        .map_or(0, |i| i + 1);
    let trimmed = &path[..end];
    let start = trimmed.iter().rposition(is_separator).map_or(0, |i| i + 1);
    trimmed[start..].to_vec()
}
