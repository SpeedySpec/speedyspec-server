use super::php::{php_addslashes, php_urlencode};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    Text(String),
    Match(usize),
}

/// A rewrite rule's query, with its `$matches[N]` references parsed once.
///
/// [`QueryTemplate::apply`] is `WP::parse_request`'s handling of a matched rule: the query after its `?`, each
/// reference replaced by the URL-encoded capture (`WP_MatchesMapRegex::apply`), then `addslashes`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryTemplate {
    parts: Vec<Part>,
}

/// `WP_MatchesMapRegex::apply( $subject, $matches )`: each `$matches[N]` replaced by the URL-encoded capture.
pub fn matches_map(subject: &str, captures: &[Option<String>]) -> String {
    QueryTemplate::references(subject).substitute(captures)
}

impl QueryTemplate {
    /// Parses `query`, dropping everything up to its first `?` (`preg_replace('!^.+\?!', '', $query)`).
    pub fn parse(query: &str) -> QueryTemplate {
        let query = match query.find('?') {
            Some(i) if i > 0 => &query[i + 1..],
            _ => query,
        };
        QueryTemplate::references(query)
    }

    /// Parses the `$matches[N]` references of `query` as written.
    fn references(query: &str) -> QueryTemplate {
        let mut parts = Vec::new();
        let mut text = String::new();
        let mut rest = query;
        while let Some(i) = rest.find("$matches[") {
            text.push_str(&rest[..i]);
            let after = &rest[i + "$matches[".len()..];
            let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
            let closes = after[digits.len()..].starts_with(']');
            if closes && !digits.is_empty() && !digits.starts_with('0') {
                if !text.is_empty() {
                    parts.push(Part::Text(std::mem::take(&mut text)));
                }
                parts.push(Part::Match(digits.parse().unwrap_or(usize::MAX)));
                rest = &after[digits.len() + 1..];
            } else {
                text.push_str("$matches[");
                rest = after;
            }
        }
        text.push_str(rest);
        if !text.is_empty() {
            parts.push(Part::Text(text));
        }
        QueryTemplate { parts }
    }

    /// Substitutes `captures` (index 0 is the whole match), as `parse_request` builds `matched_query`.
    pub fn apply(&self, captures: &[Option<String>]) -> String {
        php_addslashes(&self.substitute(captures))
    }

    fn substitute(&self, captures: &[Option<String>]) -> String {
        let mut out = String::new();
        for part in &self.parts {
            match part {
                Part::Text(text) => out.push_str(text),
                Part::Match(n) => {
                    if let Some(Some(value)) = captures.get(*n) {
                        out.push_str(&php_urlencode(value));
                    }
                }
            }
        }
        out
    }
}
