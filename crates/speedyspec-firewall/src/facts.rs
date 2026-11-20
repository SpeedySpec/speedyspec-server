use std::net::IpAddr;

use crate::ports::FirewallError;

/// What the firewall knows about a request, from its FastCGI params.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestFacts {
    /// The client's address, IPv4-mapped IPv6 turned into IPv4.
    pub client: IpAddr,
    pub method: String,
    pub path: String,
    /// The query string without its `?`; empty when there is none.
    pub query: String,
    pub user_agent: String,
    /// True when the request carries an `Authorization` header or a `wordpress_logged_in_*` cookie.
    pub has_auth: bool,
}

impl RequestFacts {
    pub fn from_params(params: &[(String, String)]) -> Result<RequestFacts, FirewallError> {
        let param = |name: &str| {
            params
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.as_str())
        };
        let remote = param("REMOTE_ADDR").ok_or_else(|| FirewallError("no REMOTE_ADDR".into()))?;
        let client = remote
            .parse::<IpAddr>()
            .map_err(|_| FirewallError(format!("REMOTE_ADDR is not an IP address: {remote:?}")))?
            .to_canonical();
        let uri = param("REQUEST_URI").unwrap_or("/");
        let (path, query) = uri.split_once('?').unwrap_or((uri, ""));
        let logged_in = param("HTTP_COOKIE").is_some_and(|cookies| {
            cookies
                .split(';')
                .any(|cookie| cookie.trim_start().starts_with("wordpress_logged_in_"))
        });
        Ok(RequestFacts {
            client,
            method: param("REQUEST_METHOD").unwrap_or("GET").to_string(),
            path: path.to_string(),
            query: query.to_string(),
            user_agent: param("HTTP_USER_AGENT").unwrap_or("").to_string(),
            has_auth: logged_in || param("HTTP_AUTHORIZATION").is_some(),
        })
    }
}
