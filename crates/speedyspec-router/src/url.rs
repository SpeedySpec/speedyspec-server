use std::fmt;

use crate::Router;
use crate::pattern::{Segment, parse};

/// Why a URL could not be generated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UrlError {
    UnknownName(String),
    MissingParam(String),
    ConstraintFailed { param: String, value: String },
}

impl fmt::Display for UrlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UrlError::UnknownName(name) => write!(f, "no route is named {name:?}"),
            UrlError::MissingParam(param) => write!(f, "missing parameter {param:?}"),
            UrlError::ConstraintFailed { param, value } => {
                write!(f, "parameter {param:?} does not accept {value:?}")
            }
        }
    }
}

impl std::error::Error for UrlError {}

/// RFC 3986 percent-encoding of everything but unreserved characters, as PHP's `rawurlencode` (and Laravel).
fn raw_url_encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

impl<H, M: Clone> Router<H, M> {
    /// The path of the route named `name` with `params` filled in, as Laravel's `route()`.
    ///
    /// Each value must pass its parameter's constraint. A trailing optional parameter with no value is dropped.
    /// Parameters the pattern does not use become the query string.
    pub fn url(&self, name: &str, params: &[(&str, &str)]) -> Result<String, UrlError> {
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.full_name().as_deref() == Some(name))
            .ok_or_else(|| UrlError::UnknownName(name.to_string()))?;
        let pattern = parse(&entry.path, &entry.wheres)
            .map_err(|_| UrlError::UnknownName(name.to_string()))?;
        let value_of = |param: &str| {
            params
                .iter()
                .find(|(key, _)| *key == param)
                .map(|(_, value)| *value)
        };
        let mut path = String::new();
        for (i, segment) in pattern.segments.iter().enumerate() {
            match segment {
                Segment::Literal(text) => {
                    let next = pattern.segments.get(i + 1);
                    let next_missing_optional = matches!(next, Some(Segment::Param { name, optional: true, .. })
                        if value_of(name).is_none());
                    let text = if next_missing_optional {
                        text.trim_end_matches('/')
                    } else {
                        text.as_str()
                    };
                    path.push_str(text);
                }
                Segment::Param {
                    name,
                    constraint,
                    optional,
                } => {
                    let Some(value) = value_of(name) else {
                        if *optional {
                            continue;
                        }
                        return Err(UrlError::MissingParam(name.clone()));
                    };
                    let accepts = regex::Regex::new(&format!("^(?:{constraint})$"))
                        .is_ok_and(|re| re.is_match(value));
                    if !accepts {
                        return Err(UrlError::ConstraintFailed {
                            param: name.clone(),
                            value: value.to_string(),
                        });
                    }
                    path.push_str(&raw_url_encode(value));
                }
            }
        }
        let extra: Vec<String> = params
            .iter()
            .filter(|(key, _)| !pattern.params.iter().any(|param| param == key))
            .map(|(key, value)| format!("{}={}", raw_url_encode(key), raw_url_encode(value)))
            .collect();
        if !extra.is_empty() {
            path.push('?');
            path.push_str(&extra.join("&"));
        }
        if path.is_empty() {
            path.push('/');
        }
        Ok(path)
    }
}
