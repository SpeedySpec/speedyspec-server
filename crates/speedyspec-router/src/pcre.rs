//! The structure of a PCRE pattern as far as matching it like PHP needs: groups, alternations and `$`, outside
//! escapes and character classes.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Token {
    Open { capturing: bool },
    Close,
    Alternation,
    Dollar,
}

/// The structural characters of `regex`, with their byte offsets.
fn tokens(regex: &str) -> Vec<(usize, Token)> {
    let bytes = regex.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 1,
            b'[' => {
                // A class: `]` right after `[` or `[^` is literal, `[:name:]` is a POSIX class.
                i += 1;
                if bytes.get(i) == Some(&b'^') {
                    i += 1;
                }
                if bytes.get(i) == Some(&b']') {
                    i += 1;
                }
                while i < bytes.len() && bytes[i] != b']' {
                    if bytes[i] == b'\\' {
                        i += 1;
                    } else if bytes[i] == b'['
                        && bytes.get(i + 1) == Some(&b':')
                        && let Some(end) = regex[i + 2..].find(":]")
                    {
                        i += end + 3;
                    }
                    i += 1;
                }
            }
            b'(' => {
                let next = bytes.get(i + 1).copied();
                let named = next == Some(b'?')
                    && matches!(bytes.get(i + 2).copied(), Some(b'P' | b'<' | b'\''))
                    && !matches!(bytes.get(i + 3).copied(), Some(b'=' | b'!'));
                out.push((
                    i,
                    Token::Open {
                        capturing: next != Some(b'?') || named,
                    },
                ));
            }
            b')' => out.push((i, Token::Close)),
            b'|' => out.push((i, Token::Alternation)),
            b'$' => out.push((i, Token::Dollar)),
            _ => {}
        }
        i += 1;
    }
    out
}

/// Whether `regex` has an alternation outside every group: then an alternative after the first is not anchored by a
/// leading `^`, and may match anywhere.
pub(crate) fn has_top_level_alternation(regex: &str) -> bool {
    let mut depth = 0usize;
    for (_, token) in tokens(regex) {
        match token {
            Token::Open { .. } => depth += 1,
            Token::Close => depth = depth.saturating_sub(1),
            Token::Alternation if depth == 0 => return true,
            _ => {}
        }
    }
    false
}

/// Whether `delimiter` occurs unescaped in `pattern`, ending a `preg_*` pattern early (PHP scans for the closing
/// delimiter skipping only backslash escapes, classes included).
pub(crate) fn ends_delimiter_early(pattern: &str, delimiter: u8) -> bool {
    let bytes = pattern.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 1,
            b if b == delimiter => return true,
            _ => {}
        }
        i += 1;
    }
    false
}

/// `regex` with each `$` matching as PCRE's does without `D`: at the end, or before a final newline.
///
/// A `$` that ends the pattern (or an alternative), outside capturing groups, becomes `\n?\z`, which the `regex`
/// crate compiles; consuming the newline changes nothing there. Any other `$` becomes the exact `(?=\n?\z)`, which
/// only `fancy-regex` compiles.
pub(crate) fn with_pcre_dollar(regex: &str) -> String {
    let tokens = tokens(regex);
    if !tokens.iter().any(|(_, t)| *t == Token::Dollar) {
        return regex.to_string();
    }
    let mut out = String::with_capacity(regex.len() + 8);
    let mut groups: Vec<bool> = Vec::new();
    let mut last = 0;
    for (at, token) in tokens {
        match token {
            Token::Open { capturing } => groups.push(capturing),
            Token::Close => {
                groups.pop();
            }
            Token::Alternation => {}
            Token::Dollar => {
                let rest = regex[at + 1..].trim_start_matches(')');
                let at_end = rest.is_empty() || rest.starts_with('|');
                let captured = groups.iter().any(|capturing| *capturing);
                out.push_str(&regex[last..at]);
                out.push_str(if at_end && !captured {
                    r"\n?\z"
                } else {
                    r"(?=\n?\z)"
                });
                last = at + 1;
            }
        }
    }
    out.push_str(&regex[last..]);
    out
}
