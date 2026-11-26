//! `get_extended()`: the split of post content at its `<!--more-->` marker.

/// The three parts `get_extended()` returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extended {
    /// The content before the marker, or all of it when there is no marker.
    pub main: Vec<u8>,
    /// The content after the marker.
    pub extended: Vec<u8>,
    /// The marker's own text: `<!--more Read on-->` gives `Read on`, after the same trimming.
    pub more_text: Vec<u8>,
}

const MARKER: &[u8] = b"<!--more";
const CLOSE: &[u8] = b"-->";

/// `get_extended( $post )`: splits `content` at the first `<!--more(.*?)?-->` (the run holds no newline), then
/// trims each part with `preg_replace( '/^[\s]*(.*)[\s]*$/', '\\1', … )`.
pub fn extended(content: &[u8]) -> Extended {
    let Some((start, text, end)) = find_marker(content) else {
        return Extended {
            main: trim_part(content),
            extended: Vec::new(),
            more_text: Vec::new(),
        };
    };
    Extended {
        main: trim_part(&content[..start]),
        extended: trim_part(&content[end..]),
        more_text: trim_part(text),
    }
}

/// The first marker that closes on its own line: its start, its text, and the offset after its `-->`.
fn find_marker(content: &[u8]) -> Option<(usize, &[u8], usize)> {
    let mut from = 0;
    while let Some(offset) = find(&content[from..], MARKER) {
        let start = from + offset;
        let text_start = start + MARKER.len();
        let line_end = content[text_start..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(content.len(), |n| text_start + n);
        // The shortest run that `-->` follows, within the line.
        if let Some(close) = find(&content[text_start..line_end], CLOSE) {
            let text_end = text_start + close;
            return Some((
                start,
                &content[text_start..text_end],
                text_end + CLOSE.len(),
            ));
        }
        from = start + 1;
    }
    None
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// PCRE's `\s`: space, tab, newline, vertical tab, form feed, carriage return.
fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r')
}

/// `preg_replace( '/^[\s]*(.*)[\s]*$/', '\\1', $part )`. Leading whitespace goes. `.` stops at a newline, so the
/// part becomes its first line when only whitespace follows that line, trailing spaces of the line included
/// (`(.*)` is greedy); otherwise the pattern does not match and the part is left exactly as it was.
fn trim_part(part: &[u8]) -> Vec<u8> {
    let lead = part.iter().take_while(|&&b| is_space(b)).count();
    let line_end = part[lead..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(part.len(), |n| lead + n);
    if part[line_end..].iter().all(|&b| is_space(b)) {
        part[lead..line_end].to_vec()
    } else {
        part.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_lines_stay_whole() {
        let parts = extended(b"  one\ntwo  ");
        assert_eq!(parts.main, b"  one\ntwo  ");
    }

    #[test]
    fn single_line_keeps_trailing_spaces() {
        let parts = extended(b" \t main  <!--more  text  -->  rest \n");
        assert_eq!(parts.main, b"main  ");
        assert_eq!(parts.more_text, b"text  ");
        assert_eq!(parts.extended, b"rest ");
    }
}
