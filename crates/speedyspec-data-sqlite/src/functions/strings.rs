//! MySQL's string functions: character positions, Unicode case, and comparisons through `speedyspec_ci`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use md5::{Digest, Md5};
use regex::{Regex, RegexBuilder};
use rusqlite::Connection;
use rusqlite::types::{Value, ValueRef};

use super::{deterministic, int, is_blob, text, user_error};
use crate::collation::fold;

/// MySQL's `SUBSTRING(s, pos[, len])` over characters: position 0 is empty, negative counts from the end.
fn substring(s: &str, pos: i64, len: Option<i64>) -> String {
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len() as i64;
    let start = match pos {
        0 => return String::new(),
        p if p > 0 => p - 1,
        p => n + p,
    };
    if start < 0 || start >= n {
        return String::new();
    }
    let end = match len {
        Some(l) if l <= 0 => return String::new(),
        Some(l) => (start + l).min(n),
        None => n,
    };
    chars[start as usize..end as usize].iter().collect()
}

fn substring_index(s: &str, delim: &str, count: i64) -> String {
    if delim.is_empty() || count == 0 {
        return String::new();
    }
    if count > 0 {
        let mut idx = 0;
        for _ in 0..count {
            match s[idx..].find(delim) {
                Some(found) => idx += found + delim.len(),
                None => return s.to_string(),
            }
        }
        s[..idx - delim.len()].to_string()
    } else {
        let mut end = s.len();
        for _ in 0..(-count) {
            match s[..end].rfind(delim) {
                Some(found) => end = found,
                None => return s.to_string(),
            }
        }
        s[end + delim.len()..].to_string()
    }
}

/// `LIKE` with an escape character over characters already folded (or not, for a BLOB).
pub(crate) fn like_match(pattern: &str, text: &str, escape: Option<char>) -> bool {
    enum Tok {
        Lit(char),
        One,
        Any,
    }
    let p: Vec<char> = pattern.chars().collect();
    let mut toks = Vec::new();
    let mut i = 0;
    while i < p.len() {
        if Some(p[i]) == escape && i + 1 < p.len() {
            toks.push(Tok::Lit(p[i + 1]));
            i += 2;
            continue;
        }
        toks.push(match p[i] {
            '%' => Tok::Any,
            '_' => Tok::One,
            c => Tok::Lit(c),
        });
        i += 1;
    }
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        match toks.get(pi) {
            Some(Tok::Lit(c)) if *c == t[ti] => {
                pi += 1;
                ti += 1;
            }
            Some(Tok::One) => {
                pi += 1;
                ti += 1;
            }
            Some(Tok::Any) => {
                star = Some((pi, ti));
                pi += 1;
            }
            _ => match star {
                Some((sp, st)) => {
                    pi = sp + 1;
                    ti = st + 1;
                    star = Some((sp, st + 1));
                }
                None => return false,
            },
        }
    }
    toks[pi..].iter().all(|t| matches!(t, Tok::Any))
}

/// Lowercase hex, as MySQL's MD5() prints it.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn fold_unless_blob(s: &str, blob: bool) -> String {
    if blob { s.to_string() } else { fold(s) }
}

pub(crate) fn register(conn: &Connection) -> rusqlite::Result<()> {
    let f = deterministic();
    conn.create_scalar_function("concat", -1, f, |ctx| {
        let mut out = String::new();
        for i in 0..ctx.len() {
            match text(ctx, i) {
                Some(s) => out.push_str(&s),
                None => return Ok(Value::Null),
            }
        }
        Ok(Value::Text(out))
    })?;
    conn.create_scalar_function("length", 1, f, |ctx| {
        Ok(match ctx.get_raw(0) {
            ValueRef::Null => Value::Null,
            ValueRef::Text(b) | ValueRef::Blob(b) => Value::Integer(b.len() as i64),
            _ => Value::Integer(text(ctx, 0).map_or(0, |s| s.len() as i64)),
        })
    })?;
    for name in ["char_length", "character_length"] {
        conn.create_scalar_function(name, 1, f, |ctx| {
            Ok(text(ctx, 0).map_or(Value::Null, |s| Value::Integer(s.chars().count() as i64)))
        })?;
    }
    for name in ["lower", "lcase"] {
        conn.create_scalar_function(name, 1, f, |ctx| {
            Ok(text(ctx, 0).map_or(Value::Null, |s| Value::Text(s.to_lowercase())))
        })?;
    }
    for name in ["upper", "ucase"] {
        conn.create_scalar_function(name, 1, f, |ctx| {
            Ok(text(ctx, 0).map_or(Value::Null, |s| Value::Text(s.to_uppercase())))
        })?;
    }
    conn.create_scalar_function("left", 2, f, |ctx| match (text(ctx, 0), int(ctx, 1)) {
        (Some(s), Some(n)) => Ok(Value::Text(s.chars().take(n.max(0) as usize).collect())),
        _ => Ok(Value::Null),
    })?;
    conn.create_scalar_function("right", 2, f, |ctx| match (text(ctx, 0), int(ctx, 1)) {
        (Some(s), Some(n)) => {
            let chars: Vec<char> = s.chars().collect();
            let skip = chars.len().saturating_sub(n.max(0) as usize);
            Ok(Value::Text(chars[skip..].iter().collect()))
        }
        _ => Ok(Value::Null),
    })?;
    for name in ["substring", "substr", "mid"] {
        for n_arg in [2, 3] {
            conn.create_scalar_function(name, n_arg, f, move |ctx| {
                let (Some(s), Some(pos)) = (text(ctx, 0), int(ctx, 1)) else {
                    return Ok(Value::Null);
                };
                let len = if n_arg == 3 {
                    match int(ctx, 2) {
                        Some(l) => Some(l),
                        None => return Ok(Value::Null),
                    }
                } else {
                    None
                };
                Ok(Value::Text(substring(&s, pos, len)))
            })?;
        }
    }
    conn.create_scalar_function("substring_index", 3, f, |ctx| {
        match (text(ctx, 0), text(ctx, 1), int(ctx, 2)) {
            (Some(s), Some(d), Some(n)) => Ok(Value::Text(substring_index(&s, &d, n))),
            _ => Ok(Value::Null),
        }
    })?;
    for n_arg in [2, 3] {
        conn.create_scalar_function("locate", n_arg, f, move |ctx| {
            let (Some(needle), Some(hay)) = (text(ctx, 0), text(ctx, 1)) else {
                return Ok(Value::Null);
            };
            let from = if n_arg == 3 {
                int(ctx, 2).unwrap_or(1).max(1) as usize
            } else {
                1
            };
            let (needle, hay): (Vec<char>, Vec<char>) = (
                fold(&needle).chars().collect(),
                hay.chars()
                    .map(|c| fold(&c.to_string()).chars().next().unwrap_or(c))
                    .collect(),
            );
            if needle.is_empty() {
                return Ok(Value::Integer(from.min(hay.len() + 1) as i64));
            }
            let found = (from - 1..hay.len().saturating_sub(needle.len() - 1))
                .find(|&i| hay[i..i + needle.len()] == needle[..]);
            Ok(Value::Integer(found.map_or(0, |i| i as i64 + 1)))
        })?;
    }
    conn.create_scalar_function("field", -1, f, |ctx| {
        let Some(target) = text(ctx, 0) else {
            return Ok(Value::Integer(0));
        };
        let target = fold(&target);
        Ok(Value::Integer(
            (1..ctx.len())
                .find(|&i| text(ctx, i).is_some_and(|s| fold(&s) == target))
                .map_or(0, |i| i as i64),
        ))
    })?;
    // CONVERT(x USING utf8mb3): MySQL's three-byte UTF-8 holds no character beyond U+FFFF, which becomes '?'.
    conn.create_scalar_function("speedyspec_utf8mb3", 1, f, |ctx| {
        Ok(match text(ctx, 0) {
            None => Value::Null,
            Some(s) => Value::Text(
                s.chars()
                    .map(|c| if u32::from(c) > 0xFFFF { '?' } else { c })
                    .collect(),
            ),
        })
    })?;
    conn.create_scalar_function("md5", 1, f, |ctx| {
        Ok(match ctx.get_raw(0) {
            ValueRef::Null => Value::Null,
            ValueRef::Text(b) | ValueRef::Blob(b) => Value::Text(hex(&Md5::digest(b))),
            _ => Value::Text(hex(&Md5::digest(
                text(ctx, 0).unwrap_or_default().as_bytes(),
            ))),
        })
    })?;
    // SQLite calls like(pattern, text[, escape]) for `text LIKE pattern [ESCAPE e]`.
    for n_arg in [2, 3] {
        conn.create_scalar_function("like", n_arg, f, move |ctx| {
            let (Some(pattern), Some(subject)) = (text(ctx, 0), text(ctx, 1)) else {
                return Ok(Value::Null);
            };
            let escape = if n_arg == 3 {
                let e = text(ctx, 2).unwrap_or_default();
                let mut chars = e.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => Some(c),
                    _ => return Err(user_error("ESCAPE expression must be a single character")),
                }
            } else {
                None
            };
            let blob = is_blob(ctx, 0) || is_blob(ctx, 1);
            let (pattern, subject) = (
                fold_unless_blob(&pattern, blob),
                fold_unless_blob(&subject, blob),
            );
            Ok(Value::Integer(i64::from(like_match(
                &pattern, &subject, escape,
            ))))
        })?;
    }
    let cache: Arc<Mutex<HashMap<(String, bool), Regex>>> = Arc::new(Mutex::new(HashMap::new()));
    // SQLite calls regexp(pattern, text) for `text REGEXP pattern`.
    conn.create_scalar_function("regexp", 2, f, move |ctx| {
        let (Some(pattern), Some(subject)) = (text(ctx, 0), text(ctx, 1)) else {
            return Ok(Value::Null);
        };
        let insensitive = !(is_blob(ctx, 0) || is_blob(ctx, 1));
        let mut cache = cache
            .lock()
            .map_err(|_| user_error("the regex cache is poisoned"))?;
        let key = (pattern.clone(), insensitive);
        if !cache.contains_key(&key) {
            let re = RegexBuilder::new(&pattern)
                .case_insensitive(insensitive)
                .build()
                .map_err(|e| user_error(e.to_string()))?;
            cache.insert(key.clone(), re);
        }
        Ok(Value::Integer(i64::from(cache[&key].is_match(&subject))))
    })?;
    Ok(())
}
