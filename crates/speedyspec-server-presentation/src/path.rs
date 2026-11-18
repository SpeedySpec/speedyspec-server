//! Normalizing a request path before it is matched against mounts.

use std::fmt;

use percent_encoding::percent_decode_str;

/// A path the server refuses outright, answered with 400.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathRejection(pub &'static str);

impl fmt::Display for PathRejection
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    {
        f.write_str(self.0)
    }
}

/// Decodes the raw path once and resolves it to the form mounts are matched against: starting with `/`, no empty or
/// `.` segments, `..` applied, a trailing `/` kept. Encoded slashes and backslashes, backslashes, NUL, invalid UTF-8
/// and `..` above the root are rejected, so no spelling of a path can reach a different mount than its plain form.
pub fn normalize(raw_path: &str) -> Result<String, PathRejection>
{
    let lower = raw_path.to_ascii_lowercase();
    if lower.contains("%2f") || lower.contains("%5c")
    {
        return Err(PathRejection("encoded slash or backslash in path"));
    }
    let decoded = percent_decode_str(raw_path)
        .decode_utf8()
        .map_err(|_| PathRejection("path is not valid UTF-8"))?;
    if decoded.contains('\\') || decoded.contains('\0')
    {
        return Err(PathRejection("backslash or NUL in path"));
    }

    let mut segments: Vec<&str> = Vec::new();
    let mut trailing = false;
    for segment in decoded.split('/')
    {
        trailing = false;
        match segment
        {
            "" => trailing = true,
            "." => trailing = true,
            ".." =>
            {
                if segments.pop().is_none()
                {
                    return Err(PathRejection("path climbs above the root"));
                }
                trailing = true;
            }
            other => segments.push(other),
        }
    }

    let mut path = String::with_capacity(decoded.len() + 1);
    path.push('/');
    path.push_str(&segments.join("/"));
    if trailing && !segments.is_empty()
    {
        path.push('/');
    }

    Ok(path)
}

#[cfg(test)]
mod tests
{
    use super::*;

    #[test]
    fn normalizes()
    {
        assert_eq!(normalize("/").unwrap(), "/");
        assert_eq!(normalize("").unwrap(), "/");
        assert_eq!(normalize("/admin/../x").unwrap(), "/x");
        assert_eq!(normalize("/%61dmin/x").unwrap(), "/admin/x");
        assert_eq!(normalize("//admin//x").unwrap(), "/admin/x");
        assert_eq!(normalize("/admin/./x/").unwrap(), "/admin/x/");
        assert_eq!(normalize("/admin/x/..").unwrap(), "/admin/");
        assert_eq!(normalize("/%C3%BCber%20men%C3%BC.html").unwrap(), "/über menü.html");
        assert_eq!(normalize("/a+b.css").unwrap(), "/a+b.css");
    }

    #[test]
    fn rejects()
    {
        for raw in ["/admin%2Fx", "/admin%2fx", "/a%5Cb", "/a%5cb", "/a%00b", "/../x", "/a/../../x", "/%C3%28", "/a\\b"]
        {
            assert!(normalize(raw).is_err(), "{raw} must be rejected");
        }
    }
}
