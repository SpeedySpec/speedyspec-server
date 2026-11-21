/// True for bytes `\[([^<>&/\[\]\x00-\x20=]++)` lets into a candidate token.
const TOKEN: [bool; 256] = {
    let mut t = [true; 256];
    let mut b = 0;
    while b <= 0x20 {
        t[b] = false;
        b += 1;
    }
    t[b'<' as usize] = false;
    t[b'>' as usize] = false;
    t[b'&' as usize] = false;
    t[b'/' as usize] = false;
    t[b'[' as usize] = false;
    t[b']' as usize] = false;
    t[b'=' as usize] = false;
    t
};

/// The tokens `do_shortcode()` collects with `preg_match_all( '@\[([^<>&/\[\]\x00-\x20=]++)@' )`: after each
/// `[`, the longest non-empty run of unreserved bytes.
///
/// A registered tag takes part in a scan only if it equals one of these whole.
pub fn candidate_tokens(content: &[u8]) -> impl Iterator<Item = &[u8]> {
    memchr::memchr_iter(b'[', content).filter_map(move |open| {
        let start = open + 1;
        let len = content[start..].iter().take_while(|&&b| TOKEN[b as usize]).count();
        (len > 0).then(|| &content[start..start + len])
    })
}
