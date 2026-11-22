//! `wp_is_internal_link()` over a fixed list of hosts.

use std::rc::Rc;

use speedyspec_wp_kses::KsesApi;
use speedyspec_wp_text_contracts::{LinkContext, TextError};

use crate::url::wp_parse_url;

/// `wp_is_internal_link()` and `wp_internal_hosts()` (`wp-includes/link-template.php`) with the hosts fixed: the
/// home URL's host, lowercased, and any more given, with no `wp_internal_hosts` filter.
pub struct HostLinkContext {
    kses: Rc<KsesApi>,
    hosts: Vec<Vec<u8>>,
}

impl HostLinkContext {
    /// The context for a site.
    ///
    /// @param kses
    ///     Supplies `wp_allowed_protocols()`.
    /// @param home
    ///     `home_url()`.
    /// @return
    ///     The context.
    pub fn new(kses: Rc<KsesApi>, home: &[u8]) -> HostLinkContext {
        let host = wp_parse_url(home).and_then(|p| p.host).unwrap_or_default();
        HostLinkContext { kses, hosts: vec![host.to_ascii_lowercase()] }
    }
}

impl LinkContext for HostLinkContext {
    fn is_internal_link(&self, url: &[u8]) -> Result<bool, TextError> {
        let link = url.to_ascii_lowercase();
        let Some(parts) = wp_parse_url(&link) else {
            return Ok(false);
        };
        let protocols = self.kses.allowed_protocols().map_err(crate::api_sanitize::kses_error)?;
        let allowed = parts.scheme.as_deref().is_some_and(|s| protocols.0.iter().any(|p| &**p == s));
        Ok(allowed && parts.host.is_some_and(|h| self.hosts.contains(&h)))
    }
}
