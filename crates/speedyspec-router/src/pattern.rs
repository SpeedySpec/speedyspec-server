//! Route patterns: `/users/{id}`, `/users/{id:[0-9]+}`, `/archive/{year}/{month?}` (Interpreter).

use std::fmt;

/// One piece of a pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    Literal(String),
    Param {
        name: String,
        constraint: String,
        optional: bool,
    },
}

/// A parsed pattern and the anchored regex that matches it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledPattern {
    pub is_static: bool,
    pub regex: String,
    pub params: Vec<String>,
    pub segments: Vec<Segment>,
}

/// Why a pattern was refused; each names the pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatternError {
    OptionalNotTrailing(String),
    CapturingGroupInConstraint(String),
    DuplicateParam(String),
    UnclosedBrace(String),
}

impl fmt::Display for PatternError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PatternError::OptionalNotTrailing(p) => {
                write!(f, "only the last segment may be optional: {p}")
            }
            PatternError::CapturingGroupInConstraint(p) => {
                write!(
                    f,
                    "a constraint may not contain a capturing group; use (?:...): {p}"
                )
            }
            PatternError::DuplicateParam(p) => write!(f, "a parameter name is used twice: {p}"),
            PatternError::UnclosedBrace(p) => write!(f, "a parameter is not closed: {p}"),
        }
    }
}

impl std::error::Error for PatternError {}

/// The constraint of a parameter with none: one path segment.
pub const DEFAULT_CONSTRAINT: &str = "[^/]+";

/// Parses `path`, taking constraints from inline `{name:regex}` or from `wheres` (inline wins).
pub fn parse(path: &str, wheres: &[(String, String)]) -> Result<CompiledPattern, PatternError> {
    let segments = split(path)?;
    let mut params: Vec<String> = Vec::new();
    let mut resolved = Vec::with_capacity(segments.len());
    for segment in segments {
        match segment {
            Segment::Literal(text) => resolved.push(Segment::Literal(text)),
            Segment::Param {
                name,
                constraint,
                optional,
            } => {
                if params.contains(&name) {
                    return Err(PatternError::DuplicateParam(path.to_string()));
                }
                let constraint = if constraint.is_empty() {
                    wheres
                        .iter()
                        .find(|(param, _)| *param == name)
                        .map(|(_, regex)| regex.clone())
                        .unwrap_or_else(|| DEFAULT_CONSTRAINT.to_string())
                } else {
                    constraint
                };
                if has_capturing_group(&constraint) {
                    return Err(PatternError::CapturingGroupInConstraint(path.to_string()));
                }
                params.push(name.clone());
                resolved.push(Segment::Param {
                    name,
                    constraint,
                    optional,
                });
            }
        }
    }

    if let Some(i) = resolved
        .iter()
        .position(|s| matches!(s, Segment::Param { optional: true, .. }))
    {
        let trailing = i + 1 == resolved.len();
        let after_slash =
            i > 0 && matches!(&resolved[i - 1], Segment::Literal(text) if text.ends_with('/'));
        if !trailing || !after_slash {
            return Err(PatternError::OptionalNotTrailing(path.to_string()));
        }
    }

    let mut regex = String::from("^");
    for (i, segment) in resolved.iter().enumerate() {
        match segment {
            Segment::Literal(text) => {
                let next_optional = matches!(
                    resolved.get(i + 1),
                    Some(Segment::Param { optional: true, .. })
                );
                let text = if next_optional {
                    &text[..text.len() - 1]
                } else {
                    text.as_str()
                };
                regex.push_str(&regex::escape(text));
            }
            Segment::Param {
                constraint,
                optional: true,
                ..
            } => regex.push_str(&format!("(?:/({constraint}))?")),
            Segment::Param { constraint, .. } => regex.push_str(&format!("({constraint})")),
        }
    }
    regex.push('$');

    Ok(CompiledPattern {
        is_static: params.is_empty(),
        regex,
        params,
        segments: resolved,
    })
}

/// Splits a path into literals and `{...}` parameters, counting nested braces inside inline constraints.
fn split(path: &str) -> Result<Vec<Segment>, PatternError> {
    let mut segments = Vec::new();
    let mut literal = String::new();
    let mut chars = path.char_indices().peekable();
    while let Some((_, c)) = chars.next() {
        if c != '{' {
            literal.push(c);
            continue;
        }
        if !literal.is_empty() {
            segments.push(Segment::Literal(std::mem::take(&mut literal)));
        }
        let mut depth = 1;
        let mut body = String::new();
        let mut escaped = false;
        for (_, c) in chars.by_ref() {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '{' {
                depth += 1;
            } else if c == '}' {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            body.push(c);
        }
        if depth != 0 {
            return Err(PatternError::UnclosedBrace(path.to_string()));
        }
        let (name, constraint) = match body.split_once(':') {
            Some((name, constraint)) => (name.to_string(), constraint.to_string()),
            None => (body, String::new()),
        };
        let (name, optional) = match name.strip_suffix('?') {
            Some(name) => (name.to_string(), true),
            None => (name, false),
        };
        segments.push(Segment::Param {
            name,
            constraint,
            optional,
        });
    }
    if !literal.is_empty() {
        segments.push(Segment::Literal(literal));
    }
    Ok(segments)
}

/// True when `regex` opens a group that captures: `(` not escaped, not in a class, and not followed by `?`.
pub fn has_capturing_group(regex: &str) -> bool {
    capturing_groups(regex) > 0
}

/// Counts the capturing groups `regex` opens.
pub fn capturing_groups(regex: &str) -> usize {
    let bytes = regex.as_bytes();
    let mut count = 0;
    let mut in_class = false;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 1,
            b'[' if !in_class => in_class = true,
            b']' if in_class => in_class = false,
            b'(' if !in_class => {
                let next = bytes.get(i + 1).copied();
                let named = next == Some(b'?')
                    && matches!(
                        bytes.get(i + 2).copied(),
                        Some(b'P') | Some(b'<') | Some(b'\'')
                    )
                    && !matches!(bytes.get(i + 3).copied(), Some(b'=') | Some(b'!'));
                if next != Some(b'?') || named {
                    count += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    count
}
