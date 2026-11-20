//! `wpdb::prepare()`, ported line by line.

use std::sync::OnceLock;

use regex::bytes::Regex;
use speedyspec_hooks::Value;

use crate::escape::{PlaceholderEscape, real_escape_value};
use crate::php::{is_scalar, to_bytes, vsprintf};

/// A `_doing_it_wrong()` notice a service hands back; the caller passes it to `WpdbHost::doing_it_wrong`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoingItWrong {
    pub function: String,
    pub message: String,
    pub version: String,
}

impl DoingItWrong {
    fn prepare(message: String, version: &str) -> DoingItWrong {
        DoingItWrong {
            function: "wpdb::prepare".into(),
            message,
            version: version.into(),
        }
    }
}

/// `prepare()`'s result: `sql` is `None` where WordPress returns null, `Some(b"")` where it returns `''`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepareOutcome {
    pub sql: Option<Vec<u8>>,
    pub notices: Vec<DoingItWrong>,
}

/// `wpdb::prepare()`.
pub struct Prepare<'a> {
    pub escape: &'a PlaceholderEscape,
    pub allow_unsafe_unquoted_parameters: bool,
}

const ALLOWED_FORMAT: &str = r"(?:[1-9][0-9]*[$])?[-+0-9]*(?: |0|'.)?[-+0-9]*(?:\.[0-9]+)?";

fn placeholder_at() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(&format!(r"(?s-u)\A(?:{ALLOWED_FORMAT})?[sdfFi]")).unwrap())
}

fn split_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(&format!(
            r"(?s-u)(^|[^%]|(?:%%)+)(%(?:{ALLOWED_FORMAT})?[sdfFi])"
        ))
        .unwrap()
    })
}

/// `preg_replace("/%(?:%|$|(?!($allowed_format)?[sdfFi]))/", '%%\1', $query)`: every `%` that does not start a
/// placeholder becomes `%%` (the capture in the negative look-ahead is always empty). Written as a scan because
/// `regex` has no look-ahead; PCRE's `$` also matches before a final newline.
fn escape_unrecognised(query: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(query.len() + 4);
    let mut i = 0;
    while i < query.len() {
        if query[i] != b'%' {
            out.push(query[i]);
            i += 1;
            continue;
        }
        let rest = &query[i + 1..];
        if rest.first() == Some(&b'%') {
            out.extend_from_slice(b"%%");
            i += 2;
        } else if rest.is_empty() || rest == b"\n" || !placeholder_at().is_match(rest) {
            out.extend_from_slice(b"%%");
            i += 1;
        } else {
            out.push(b'%');
            i += 1;
        }
    }
    out
}

/// `preg_split(.., PREG_SPLIT_DELIM_CAPTURE)` with the two capture groups: text, glue, placeholder, text, ...
fn split(query: &[u8]) -> Vec<Vec<u8>> {
    let mut parts = Vec::new();
    let mut last = 0;
    for caps in split_pattern().captures_iter(query) {
        let whole = caps.get(0).expect("group 0");
        parts.push(query[last..whole.start()].to_vec());
        parts.push(caps.get(1).map_or(&[][..], |m| m.as_bytes()).to_vec());
        parts.push(caps.get(2).map_or(&[][..], |m| m.as_bytes()).to_vec());
        last = whole.end();
    }
    parts.push(query[last..].to_vec());
    parts
}

/// The argnum of a numbered format (`1$` → 0), as `(int) substr($format, 0, $pos) - 1`.
fn argnum(format: &[u8]) -> Option<i64> {
    let pos = format.iter().position(|&b| b == b'$')?;
    Some(
        std::str::from_utf8(&format[..pos])
            .ok()?
            .parse::<i64>()
            .unwrap_or(0)
            - 1,
    )
}

/// `(int) substr($placeholder, 1)`.
fn leading_int(bytes: &[u8]) -> i64 {
    let digits: Vec<u8> = bytes
        .iter()
        .copied()
        .take_while(u8::is_ascii_digit)
        .collect();
    std::str::from_utf8(&digits)
        .ok()
        .and_then(|d| d.parse().ok())
        .unwrap_or(0)
}

fn gettype(v: &Value) -> &'static str {
    match v {
        Value::Null => "NULL",
        Value::Bool(_) => "boolean",
        Value::Int(_) => "integer",
        Value::Float(_) => "double",
        Value::Str(_) | Value::Bytes(_) => "string",
        Value::Array(_) => "array",
        Value::Opaque(_) => "object",
    }
}

impl Prepare<'_> {
    pub fn prepare(&self, query: Option<&[u8]>, args: &[Value]) -> PrepareOutcome {
        let mut notices = Vec::new();
        let Some(query) = query else {
            return PrepareOutcome { sql: None, notices };
        };
        if !query.contains(&b'%') {
            notices.push(DoingItWrong::prepare(
                "The query argument of wpdb::prepare() must have a placeholder.".into(),
                "3.9.0",
            ));
        }
        let query = replace(&replace(query, b"'%s'", b"%s"), b"\"%s\"", b"%s");
        let query = escape_unrecognised(&query);
        let split_query = split(&query);
        let split_count = split_query.len();
        let mut placeholder_count = ((split_count - 1) / 3) as i64;

        let spread;
        let mut passed_as_array = false;
        let args: &[Value] = match args {
            [Value::Array(items)] => {
                passed_as_array = true;
                spread = items.iter().map(|(_, v)| v.clone()).collect::<Vec<_>>();
                &spread
            }
            other => other,
        };

        let mut new_query = Vec::with_capacity(query.len() + 16);
        let mut key = 2;
        let mut arg_id: i64 = 0;
        let mut arg_identifiers = Vec::new();
        let mut arg_strings = Vec::new();
        while key < split_count {
            let original = &split_query[key];
            let format = &original[1..original.len() - 1];
            let mut ty = original[original.len() - 1];
            let mut placeholder = original.clone();
            let prev = &split_query[key - 1];
            if ty == b'f' && self.allow_unsafe_unquoted_parameters && prev.last() == Some(&b'%') {
                let s: Vec<u8> = [split_query[key - 2].as_slice(), prev.as_slice()].concat();
                let (mut k, l) = (1usize, s.len());
                while k <= l && s[l - k] == b'%' {
                    k += 1;
                }
                placeholder = [
                    b"%".as_slice(),
                    if k % 2 == 1 { b"%" } else { b"" },
                    format,
                    &[ty],
                ]
                .concat();
                placeholder_count -= 1;
            } else {
                if ty == b'f' {
                    ty = b'F';
                    placeholder = [b"%".as_slice(), format, b"F"].concat();
                }
                if ty == b'i' {
                    placeholder = [b"`%".as_slice(), format, b"s`"].concat();
                    arg_identifiers.push(argnum(format).unwrap_or(arg_id));
                } else if ty != b'd' && ty != b'F' {
                    arg_strings.push(argnum(format).unwrap_or(arg_id));
                    if !self.allow_unsafe_unquoted_parameters
                        || (format.is_empty() && prev.last() != Some(&b'%'))
                    {
                        placeholder = [b"'%".as_slice(), format, b"s'"].concat();
                    }
                }
            }
            new_query.extend_from_slice(&split_query[key - 2]);
            new_query.extend_from_slice(prev);
            new_query.extend_from_slice(&placeholder);
            key += 3;
            arg_id += 1;
        }
        new_query.extend_from_slice(&split_query[key - 2]);

        let dual_use: Vec<i64> = arg_identifiers
            .iter()
            .copied()
            .filter(|i| arg_strings.contains(i))
            .collect();
        if !dual_use.is_empty() {
            let mut used: Vec<(i64, Vec<String>)> = Vec::new();
            let (mut key, mut arg_id) = (2, 0i64);
            while key < split_count {
                let placeholder = &split_query[key];
                let pos = argnum(&placeholder[1..placeholder.len() - 1]).unwrap_or(arg_id);
                let text = String::from_utf8_lossy(placeholder).into_owned();
                match used.iter_mut().find(|(p, _)| *p == pos) {
                    Some((_, list)) => list.push(text),
                    None => used.push((pos, vec![text])),
                }
                key += 3;
                arg_id += 1;
            }
            let conflicts: Vec<String> = dual_use
                .iter()
                .filter_map(|pos| {
                    used.iter()
                        .find(|(p, _)| p == pos)
                        .map(|(_, l)| l.join(" and "))
                })
                .collect();
            notices.push(DoingItWrong::prepare(
                format!(
                    "Arguments cannot be prepared as both an Identifier and Value. Found the following conflicts: {}",
                    conflicts.join(", ")
                ),
                "6.2.0",
            ));
            return PrepareOutcome { sql: None, notices };
        }

        let args_count = args.len() as i64;
        if args_count != placeholder_count {
            if placeholder_count == 1 && passed_as_array {
                notices.push(DoingItWrong::prepare(
                    "The query only expected one placeholder, but an array of multiple placeholders was sent.".into(),
                    "4.9.0",
                ));
                return PrepareOutcome { sql: None, notices };
            }
            notices.push(DoingItWrong::prepare(
                format!(
                    "The query does not contain the correct number of placeholders ({placeholder_count}) for the number of arguments passed ({args_count})."
                ),
                "4.8.3",
            ));
            if args_count < placeholder_count {
                let max_numbered = (2..split_count)
                    .step_by(3)
                    .map(|i| leading_int(&split_query[i][1..]))
                    .max()
                    .unwrap_or(0);
                if max_numbered == 0 || args_count < max_numbered {
                    return PrepareOutcome {
                        sql: Some(Vec::new()),
                        notices,
                    };
                }
            }
        }

        let mut escaped = Vec::with_capacity(args.len());
        for (i, value) in args.iter().enumerate() {
            if arg_identifiers.contains(&(i as i64)) {
                let bytes = to_bytes(value).unwrap_or_default();
                escaped.push(crate::escape::string_value(replace(&bytes, b"`", b"``")));
            } else if matches!(value, Value::Int(_) | Value::Float(_)) {
                escaped.push(value.clone());
            } else {
                let value = if !is_scalar(value) && !matches!(value, Value::Null) {
                    notices.push(DoingItWrong::prepare(
                        format!("Unsupported value type ({}).", gettype(value)),
                        "4.8.2",
                    ));
                    Value::Str(String::new())
                } else {
                    value.clone()
                };
                escaped.push(crate::escape::string_value(real_escape_value(
                    &value,
                    self.escape,
                )));
            }
        }
        // PHP's vsprintf throws when an argument is missing; `None` stands for that.
        let sql = vsprintf(&new_query, &escaped).map(|q| self.escape.add_bytes(&q));
        PrepareOutcome { sql, notices }
    }
}

pub(crate) fn replace(haystack: &[u8], needle: &[u8], with: &[u8]) -> Vec<u8> {
    if needle.is_empty() {
        return haystack.to_vec();
    }
    let mut out = Vec::with_capacity(haystack.len());
    let mut i = 0;
    while i < haystack.len() {
        if haystack[i..].starts_with(needle) {
            out.extend_from_slice(with);
            i += needle.len();
        } else {
            out.push(haystack[i]);
            i += 1;
        }
    }
    out
}
