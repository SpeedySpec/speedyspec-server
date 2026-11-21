use regex::Regex;

use crate::facts::RequestFacts;
use crate::settings::SettingsIssue;

/// The part of a request a pattern rule looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Path,
    Query,
    UserAgent,
}

/// "Refuse with `status` when `field` matches `regex`" (a Specification over requests).
#[derive(Debug, Clone)]
pub struct PatternRule {
    pub field: Field,
    pub status: u16,
    pub regex: Regex,
}

impl PatternRule {
    /// Lines of `<field> <status> <regex>`: field is `path`, `query` or `ua`; status is `403` or `429`. Blank
    /// lines and lines starting with `#` are ignored; a malformed line is skipped and reported.
    pub fn parse_lines(text: &str) -> (Vec<PatternRule>, Vec<SettingsIssue>) {
        let mut rules = Vec::new();
        let mut issues = Vec::new();
        for (index, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            match parse_line(line) {
                Ok(rule) => rules.push(rule),
                Err(message) => issues.push(SettingsIssue {
                    setting: "patterns",
                    line: index + 1,
                    message,
                }),
            }
        }
        (rules, issues)
    }

    pub fn matches(&self, facts: &RequestFacts) -> bool {
        let text = match self.field {
            Field::Path => &facts.path,
            Field::Query => &facts.query,
            Field::UserAgent => &facts.user_agent,
        };
        self.regex.is_match(text)
    }
}

fn parse_line(line: &str) -> Result<PatternRule, String> {
    let mut parts = line.splitn(3, char::is_whitespace);
    let (field, status, pattern) = match (parts.next(), parts.next(), parts.next()) {
        (Some(f), Some(s), Some(p)) if !p.trim().is_empty() => (f, s, p.trim()),
        _ => return Err(format!("expected `<field> <status> <regex>`: {line:?}")),
    };
    let field = match field {
        "path" => Field::Path,
        "query" => Field::Query,
        "ua" => Field::UserAgent,
        other => return Err(format!("unknown field {other:?}; use path, query or ua")),
    };
    let status = match status {
        "403" => 403,
        "429" => 429,
        other => return Err(format!("unsupported status {other:?}; use 403 or 429")),
    };
    let regex = Regex::new(pattern).map_err(|error| format!("bad regex {pattern:?}: {error}"))?;
    Ok(PatternRule {
        field,
        status,
        regex,
    })
}
