//! Who a request is from and which host it names, honouring proxy headers only from trusted proxies.

use std::net::IpAddr;

use http::HeaderMap;
use http::header::{FORWARDED, HOST};
use ipnet::IpNet;

/// The client and host a request is really for, after proxy headers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestOrigin
{
    pub client: IpAddr,
    /// Lowercase, without port, brackets or a trailing dot. Empty when the request names no host.
    pub host: String,
    pub https: bool,
}

/// Reads the origin of a request from `peer`. Only when `peer` is in `trusted` are `Forwarded` (preferred) or
/// `X-Forwarded-For` / `-Host` / `-Proto` applied.
pub fn origin(peer: IpAddr, headers: &HeaderMap, tls: bool, trusted: &[IpNet]) -> RequestOrigin
{
    let host = headers.get(HOST).and_then(|v| v.to_str().ok()).map(normalize_host).unwrap_or_default();
    let mut seen = RequestOrigin { client: peer, host, https: tls };
    if !is_trusted(peer, trusted)
    {
        return seen;
    }

    if let Some(value) = joined(headers, FORWARDED.as_str())
    {
        let elements: Vec<Vec<(String, String)>> = value.split(',').map(pairs).collect();
        let mut chosen = None;
        for element in elements.iter().rev()
        {
            if let Some(ip) = find(element, "for").and_then(parse_node)
            {
                chosen = Some(ip);
                if !is_trusted(ip, trusted)
                {
                    break;
                }
            }
        }
        if let Some(ip) = chosen
        {
            seen.client = ip;
        }
        if let Some(host) = elements.iter().rev().find_map(|e| find(e, "host"))
        {
            seen.host = normalize_host(&host);
        }
        if let Some(proto) = elements.iter().rev().find_map(|e| find(e, "proto"))
        {
            seen.https = proto.eq_ignore_ascii_case("https");
        }
        return seen;
    }

    if let Some(value) = joined(headers, "x-forwarded-for")
    {
        let addresses: Vec<IpAddr> = value.split(',').filter_map(|a| a.trim().parse().ok()).collect();
        if let Some(ip) = addresses.iter().rev().find(|ip| !is_trusted(**ip, trusted)).or(addresses.first())
        {
            seen.client = *ip;
        }
    }
    if let Some(value) = joined(headers, "x-forwarded-host")
        && let Some(first) = value.split(',').next()
    {
        seen.host = normalize_host(first);
    }
    if let Some(value) = joined(headers, "x-forwarded-proto")
        && let Some(first) = value.split(',').next()
    {
        seen.https = first.trim().eq_ignore_ascii_case("https");
    }

    seen
}

fn is_trusted(ip: IpAddr, trusted: &[IpNet]) -> bool
{
    trusted.iter().any(|net| net.contains(&ip))
}

/// Every value of a header, joined with commas, as one list.
fn joined(headers: &HeaderMap, name: &str) -> Option<String>
{
    let values: Vec<&str> = headers.get_all(name).iter().filter_map(|v| v.to_str().ok()).collect();
    (!values.is_empty()).then(|| values.join(","))
}

/// `key=value` pairs of one `Forwarded` element, keys lowercased, quotes removed.
fn pairs(element: &str) -> Vec<(String, String)>
{
    element
        .split(';')
        .filter_map(|pair| pair.split_once('='))
        .map(|(key, value)| (key.trim().to_ascii_lowercase(), value.trim().trim_matches('"').to_string()))
        .collect()
}

fn find(element: &[(String, String)], key: &str) -> Option<String>
{
    element.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
}

/// A `Forwarded: for=` node: `1.2.3.4`, `1.2.3.4:80`, `[v6]` or `[v6]:80`. Obfuscated and `unknown` nodes give `None`.
fn parse_node(node: String) -> Option<IpAddr>
{
    if let Some(rest) = node.strip_prefix('[')
    {
        return rest.split(']').next()?.parse().ok();
    }
    node.parse().ok().or_else(|| node.rsplit_once(':').and_then(|(ip, _)| ip.parse().ok()))
}

/// Lowercase, without port, IPv6 brackets or one trailing dot.
fn normalize_host(raw: &str) -> String
{
    let raw = raw.trim();
    let host = if let Some(rest) = raw.strip_prefix('[')
    {
        rest.split(']').next().unwrap_or("")
    }
    else
    {
        match raw.rsplit_once(':')
        {
            Some((name, port)) if !name.contains(':') && port.chars().all(|c| c.is_ascii_digit()) => name,
            _ => raw,
        }
    };
    let host = host.strip_suffix('.').unwrap_or(host);

    host.to_ascii_lowercase()
}

#[cfg(test)]
mod tests
{
    use http::HeaderValue;

    use super::*;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap
    {
        let mut map = HeaderMap::new();
        for (name, value) in pairs
        {
            map.append(*name, HeaderValue::from_str(value).unwrap());
        }
        map
    }

    fn trusted() -> Vec<IpNet>
    {
        vec!["10.0.0.0/8".parse().unwrap(), "127.0.0.1/32".parse().unwrap()]
    }

    const PEER: IpAddr = IpAddr::V4(std::net::Ipv4Addr::new(10, 0, 0, 5));
    const STRANGER: IpAddr = IpAddr::V4(std::net::Ipv4Addr::new(192, 0, 2, 1));

    #[test]
    fn host_spellings()
    {
        assert_eq!(origin(STRANGER, &headers(&[("host", "Example.COM:443")]), false, &[]).host, "example.com");
        assert_eq!(origin(STRANGER, &headers(&[("host", "example.com.")]), false, &[]).host, "example.com");
        assert_eq!(origin(STRANGER, &headers(&[("host", "[::1]:8080")]), false, &[]).host, "::1");
        assert_eq!(origin(STRANGER, &headers(&[("host", "[::1]")]), false, &[]).host, "::1");
        assert_eq!(origin(STRANGER, &headers(&[]), false, &[]).host, "");
    }

    #[test]
    fn untrusted_forwarded_headers_ignored()
    {
        let map = headers(&[
            ("host", "example.com"),
            ("x-forwarded-host", "admin.example.com"),
            ("x-forwarded-for", "203.0.113.9"),
            ("x-forwarded-proto", "https"),
            ("forwarded", "for=198.51.100.7;host=a.example;proto=https"),
        ]);
        let seen = origin(STRANGER, &map, false, &trusted());
        assert_eq!(seen, RequestOrigin { client: STRANGER, host: "example.com".to_string(), https: false });
    }

    #[test]
    fn trusted_x_forwarded()
    {
        let map = headers(&[
            ("host", "internal"),
            ("x-forwarded-host", "Admin.Example.com"),
            ("x-forwarded-for", "203.0.113.9, 10.0.0.7"),
            ("x-forwarded-proto", "https"),
        ]);
        let seen = origin(PEER, &map, false, &trusted());
        assert_eq!(seen.client, "203.0.113.9".parse::<IpAddr>().unwrap());
        assert_eq!(seen.host, "admin.example.com");
        assert!(seen.https);
    }

    #[test]
    fn trusted_forwarded_wins()
    {
        let map = headers(&[
            ("host", "internal"),
            ("forwarded", "for=198.51.100.7;host=a.example;proto=https"),
            ("x-forwarded-for", "203.0.113.9"),
        ]);
        let seen = origin(PEER, &map, false, &trusted());
        assert_eq!(seen.client, "198.51.100.7".parse::<IpAddr>().unwrap());
        assert_eq!(seen.host, "a.example");
        assert!(seen.https);
    }

    #[test]
    fn forwarded_quoted_ipv6()
    {
        let map = headers(&[("forwarded", "for=\"[2001:db8::1]:4711\""), ("host", "example.com")]);
        let seen = origin(PEER, &map, true, &trusted());
        assert_eq!(seen.client, "2001:db8::1".parse::<IpAddr>().unwrap());
        assert!(seen.https);
    }
}
