//! The rules `wpdb` applies to query text by regex: its class and its table.

use std::sync::OnceLock;

use regex::bytes::Regex;

/// What `query()` sets and returns, decided from the text as WordPress decides it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryClass {
    Ddl,
    Write { insert: bool },
    Read,
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("WordPress's patterns are valid"))
}

impl QueryClass {
    pub fn of(query: &[u8]) -> QueryClass {
        static DDL: OnceLock<Regex> = OnceLock::new();
        static WRITE: OnceLock<Regex> = OnceLock::new();
        static INSERT: OnceLock<Regex> = OnceLock::new();
        if re(&DDL, r"(?i-u)^\s*(create|alter|truncate|drop)\s").is_match(query) {
            QueryClass::Ddl
        } else if re(&WRITE, r"(?i-u)^\s*(insert|delete|update|replace)\s").is_match(query) {
            QueryClass::Write {
                insert: re(&INSERT, r"(?i-u)^\s*(insert|replace)\s").is_match(query),
            }
        } else {
            QueryClass::Read
        }
    }
}

/// `preg_replace('/\((?!\s*select)[^(]*?\)/is', '()', $query)`, as a scan (`regex` has no look-ahead): a `(` not
/// followed by `select` and closed before another `(` opens is emptied.
fn strip_parentheses(query: &[u8]) -> Vec<u8> {
    static SELECT: OnceLock<Regex> = OnceLock::new();
    let select = re(&SELECT, r"(?is-u)\A\s*select");
    let mut out = Vec::with_capacity(query.len());
    let mut i = 0;
    while i < query.len() {
        if query[i] == b'(' && !select.is_match(&query[i + 1..]) {
            let close = query[i + 1..].iter().position(|&b| b == b'(' || b == b')');
            if let Some(offset) = close.filter(|&o| query[i + 1 + o] == b')') {
                out.extend_from_slice(b"()");
                i += offset + 2;
                continue;
            }
        }
        out.push(query[i]);
        i += 1;
    }
    out
}

fn without_backticks(name: &[u8]) -> Vec<u8> {
    name.iter().copied().filter(|&b| b != b'`').collect()
}

/// `wpdb::get_table_from_query`, every pattern in its order; `None` where it returns false.
pub fn get_table_from_query(query: &[u8]) -> Option<Vec<u8>> {
    static SET_STATEMENT: OnceLock<Regex> = OnceLock::new();
    static COMMON: OnceLock<Regex> = OnceLock::new();
    static SHOW_WHERE: OnceLock<Regex> = OnceLock::new();
    static SHOW_LIKE: OnceLock<Regex> = OnceLock::new();
    static REST: OnceLock<Regex> = OnceLock::new();
    const NAME: &str = r"(?:[0-9a-zA-Z$_.`-]|[\xC2-\xDF][\x80-\xBF])+";

    let end = query
        .iter()
        .rposition(|b| !b";/-#".contains(b))
        .map_or(0, |p| p + 1);
    let query = &query[..end];
    let start = query
        .iter()
        .position(|b| !b"\r\n\t (".contains(b))
        .unwrap_or(query.len());
    let query = strip_parentheses(&query[start..]);
    let query = re(&SET_STATEMENT, r"(?is-u)^SET STATEMENT.+?\sFOR\s+")
        .replace(&query, &b""[..])
        .into_owned();

    let common = re(
        &COMMON,
        &format!(
            r"(?is-u)^\s*(?:SELECT.*?\s+FROM|INSERT(?:\s+LOW_PRIORITY|\s+DELAYED|\s+HIGH_PRIORITY)?(?:\s+IGNORE)?(?:\s+INTO)?|REPLACE(?:\s+LOW_PRIORITY|\s+DELAYED)?(?:\s+INTO)?|UPDATE(?:\s+LOW_PRIORITY)?(?:\s+IGNORE)?|DELETE(?:\s+LOW_PRIORITY|\s+QUICK|\s+IGNORE)*(?:.+?FROM)?)\s+({NAME})"
        ),
    );
    if let Some(c) = common.captures(&query) {
        return Some(without_backticks(&c[1]));
    }
    // `("|')name\1`, written as two alternatives: `regex` has no back-references.
    let plain = r"(?:[0-9a-zA-Z$_.-]|[\xC2-\xDF][\x80-\xBF])+";
    let show_where = re(
        &SHOW_WHERE,
        &format!(
            r#"(?is-u)^\s*SHOW\s+(?:TABLE\s+STATUS|(?:FULL\s+)?TABLES).+WHERE\s+Name\s*=\s*(?:"({plain})"|'({plain})')"#
        ),
    );
    if let Some(c) = show_where.captures(&query) {
        return c.get(1).or(c.get(2)).map(|m| m.as_bytes().to_vec());
    }
    let escaped = r"(?:[\\0-9a-zA-Z$_.-]|[\xC2-\xDF][\x80-\xBF])+";
    let show_like = re(
        &SHOW_LIKE,
        &format!(
            r#"(?is-u)^\s*SHOW\s+(?:TABLE\s+STATUS|(?:FULL\s+)?TABLES)\s+(?:WHERE\s+Name\s+)?LIKE\s*(?:"({escaped})%?"|'({escaped})%?')"#
        ),
    );
    if let Some(c) = show_like.captures(&query) {
        let name = c
            .get(1)
            .or(c.get(2))
            .map(|m| m.as_bytes())
            .unwrap_or_default();
        return Some(crate::prepare::replace(name, b"\\_", b"_"));
    }
    let rest = re(
        &REST,
        &format!(
            r"(?is-u)^\s*(?:(?:EXPLAIN\s+(?:EXTENDED\s+)?)?SELECT.*?\s+FROM|DESCRIBE|DESC|EXPLAIN|HANDLER|(?:LOCK|UNLOCK)\s+TABLE(?:S)?|(?:RENAME|OPTIMIZE|BACKUP|RESTORE|CHECK|CHECKSUM|ANALYZE|REPAIR).*\s+TABLE|TRUNCATE(?:\s+TABLE)?|CREATE(?:\s+TEMPORARY)?\s+TABLE(?:\s+IF\s+NOT\s+EXISTS)?|ALTER(?:\s+IGNORE)?\s+TABLE|DROP\s+TABLE(?:\s+IF\s+EXISTS)?|CREATE(?:\s+\w+)?\s+INDEX.*\s+ON|DROP\s+INDEX.*\s+ON|LOAD\s+DATA.*INFILE.*INTO\s+TABLE|(?:GRANT|REVOKE).*ON\s+TABLE|SHOW\s+(?:.*FROM|.*TABLE))\s+\(*\s*({NAME})\s*\)*"
        ),
    );
    rest.captures(&query).map(|c| without_backticks(&c[1]))
}
