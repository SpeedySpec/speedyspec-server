use std::str::FromStr;

use speedyspec_http::{HttpError, Uri};

use crate::percent::encode;

/// A URI with PSR-7's normalization: scheme and host lowercased, a scheme's default port dropped, components
/// percent-encoded once.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HttpUri {
    scheme: String,
    user_info: String,
    host: String,
    port: Option<u16>,
    path: String,
    query: String,
    fragment: String,
}

fn default_port(scheme: &str) -> Option<u16> {
    match scheme {
        "http" => Some(80),
        "https" => Some(443),
        _ => None,
    }
}

impl HttpUri {
    fn normalized(mut self) -> HttpUri {
        if self.port.is_some() && self.port == default_port(&self.scheme) {
            self.port = None;
        }
        self
    }
}

impl FromStr for HttpUri {
    type Err = HttpError;

    /// Splits a URI reference into its RFC 3986 components (appendix B).
    fn from_str(input: &str) -> Result<HttpUri, HttpError> {
        if input.chars().any(|c| c.is_ascii_control() || c == ' ') {
            return Err(HttpError::InvalidUri(input.to_string()));
        }
        let mut rest = input;
        let mut uri = HttpUri::default();
        if let Some(i) = rest.find('#') {
            uri.fragment = encode(&rest[i + 1..], b":@/?");
            rest = &rest[..i];
        }
        if let Some(i) = rest.find('?') {
            uri.query = encode(&rest[i + 1..], b":@/?");
            rest = &rest[..i];
        }
        if let Some(i) = rest.find(':')
            && !rest[..i].contains('/')
            && rest[..i]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic())
        {
            uri.scheme = rest[..i].to_ascii_lowercase();
            rest = &rest[i + 1..];
        }
        if let Some(after) = rest.strip_prefix("//") {
            let end = after.find('/').unwrap_or(after.len());
            let mut authority = &after[..end];
            rest = &after[end..];
            if let Some(i) = authority.rfind('@') {
                uri.user_info = authority[..i].to_string();
                authority = &authority[i + 1..];
            }
            let (host, port) = match authority.rfind(':') {
                Some(i)
                    if !authority[i + 1..].is_empty()
                        && authority[i + 1..].bytes().all(|b| b.is_ascii_digit()) =>
                {
                    let port = authority[i + 1..]
                        .parse::<u16>()
                        .map_err(|_| HttpError::InvalidUri(input.to_string()))?;
                    (&authority[..i], Some(port))
                }
                _ => (authority, None),
            };
            uri.host = host.to_ascii_lowercase();
            uri.port = port;
        }
        uri.path = encode(rest, b":@/");
        Ok(uri.normalized())
    }
}

impl Uri for HttpUri {
    fn scheme(&self) -> &str {
        &self.scheme
    }

    fn authority(&self) -> String {
        if self.host.is_empty() {
            return String::new();
        }
        let mut out = String::new();
        if !self.user_info.is_empty() {
            out.push_str(&self.user_info);
            out.push('@');
        }
        out.push_str(&self.host);
        if let Some(port) = self.port {
            out.push_str(&format!(":{port}"));
        }
        out
    }

    fn user_info(&self) -> &str {
        &self.user_info
    }

    fn host(&self) -> &str {
        &self.host
    }

    fn port(&self) -> Option<u16> {
        self.port
    }

    fn path(&self) -> &str {
        &self.path
    }

    fn query(&self) -> &str {
        &self.query
    }

    fn fragment(&self) -> &str {
        &self.fragment
    }

    /// Composes the URI as PSR-7's `__toString`: a rootless path gets a `/` when there is an authority.
    fn to_uri_string(&self) -> String {
        let mut out = String::new();
        if !self.scheme.is_empty() {
            out.push_str(&self.scheme);
            out.push(':');
        }
        let authority = self.authority();
        if !authority.is_empty() || self.scheme == "file" {
            out.push_str("//");
            out.push_str(&authority);
        }
        if !authority.is_empty() && !self.path.is_empty() && !self.path.starts_with('/') {
            out.push('/');
        }
        out.push_str(&self.path);
        if !self.query.is_empty() {
            out.push('?');
            out.push_str(&self.query);
        }
        if !self.fragment.is_empty() {
            out.push('#');
            out.push_str(&self.fragment);
        }
        out
    }

    fn with_scheme(mut self, scheme: &str) -> Result<HttpUri, HttpError> {
        if !scheme.is_empty()
            && !scheme
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"+-.".contains(&b))
        {
            return Err(HttpError::InvalidUri(scheme.to_string()));
        }
        self.scheme = scheme.to_ascii_lowercase();
        Ok(self.normalized())
    }

    fn with_user_info(mut self, user: &str, password: Option<&str>) -> HttpUri {
        self.user_info = match password {
            Some(password) if !user.is_empty() => format!("{user}:{password}"),
            _ => user.to_string(),
        };
        self
    }

    fn with_host(mut self, host: &str) -> HttpUri {
        self.host = host.to_ascii_lowercase();
        self
    }

    fn with_port(mut self, port: Option<u16>) -> HttpUri {
        self.port = port;
        self.normalized()
    }

    fn with_path(mut self, path: &str) -> HttpUri {
        self.path = encode(path, b":@/");
        self
    }

    fn with_query(mut self, query: &str) -> HttpUri {
        self.query = encode(query.strip_prefix('?').unwrap_or(query), b":@/?");
        self
    }

    fn with_fragment(mut self, fragment: &str) -> HttpUri {
        self.fragment = encode(fragment.strip_prefix('#').unwrap_or(fragment), b":@/?");
        self
    }
}
