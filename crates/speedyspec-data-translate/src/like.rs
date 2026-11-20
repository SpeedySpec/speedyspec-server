//! MySQL's `LIKE`: `%` any run, `_` one character, `\` escaping either.

/// Whether `text` matches `pattern` as MySQL's `LIKE` with its default `ESCAPE '\'`.
pub fn mysql_like(pattern: &str, text: &str, case_insensitive: bool) -> bool {
    let fold = |s: &str| {
        if case_insensitive {
            s.to_lowercase()
        } else {
            s.to_string()
        }
    };
    let (pattern, text): (Vec<char>, Vec<char>) = (
        fold(pattern).chars().collect(),
        fold(text).chars().collect(),
    );
    // Tokens: Some(c) a literal, None for `_`, and `%` kept apart.
    enum Tok {
        Lit(char),
        One,
        Any,
    }
    let mut toks = Vec::new();
    let mut i = 0;
    while i < pattern.len() {
        match pattern[i] {
            '\\' if i + 1 < pattern.len() => {
                toks.push(Tok::Lit(pattern[i + 1]));
                i += 1;
            }
            '%' => toks.push(Tok::Any),
            '_' => toks.push(Tok::One),
            c => toks.push(Tok::Lit(c)),
        }
        i += 1;
    }
    // Greedy matching with backtracking to the last `%`.
    let (mut p, mut t) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        match toks.get(p) {
            Some(Tok::Lit(c)) if *c == text[t] => {
                p += 1;
                t += 1;
            }
            Some(Tok::One) => {
                p += 1;
                t += 1;
            }
            Some(Tok::Any) => {
                star = Some((p, t));
                p += 1;
            }
            _ => match star {
                Some((sp, st)) => {
                    p = sp + 1;
                    t = st + 1;
                    star = Some((sp, st + 1));
                }
                None => return false,
            },
        }
    }
    toks[p..].iter().all(|t| matches!(t, Tok::Any))
}
