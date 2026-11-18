//! Placeholder access checks, used until the sign-in project provides the real one.

use std::future::Future;
use std::pin::Pin;

use speedyspec_server_domain::{Access, AccessCheck, AccessDecision, AccessRequest, Identity};

/// Denies every request to a `signed-in` mount and lets `public` ones through. The default.
#[derive(Clone, Copy, Debug, Default)]
pub struct DenySignedIn;

impl AccessCheck for DenySignedIn
{
    fn check<'a>(&'a self, request: AccessRequest<'a>) -> Pin<Box<dyn Future<Output = AccessDecision> + Send + 'a>>
    {
        let decision = match request.access
        {
            Access::Public => AccessDecision::Allowed(Identity::anonymous()),
            Access::SignedIn => AccessDecision::Denied,
        };
        Box::pin(async move { decision })
    }
}

/// Lets everything through. Only for local development: the server refuses to start with it on a listener that is not
/// a loopback address.
#[derive(Clone, Copy, Debug, Default)]
pub struct DevAllowAll;

impl AccessCheck for DevAllowAll
{
    fn check<'a>(&'a self, _request: AccessRequest<'a>) -> Pin<Box<dyn Future<Output = AccessDecision> + Send + 'a>>
    {
        Box::pin(async { AccessDecision::Allowed(Identity::anonymous()) })
    }
}

#[cfg(test)]
mod tests
{
    use super::*;

    async fn decide(check: &dyn AccessCheck, access: Access) -> AccessDecision
    {
        let headers = http::HeaderMap::new();
        check.check(AccessRequest { access, headers: &headers, client: [127, 0, 0, 1].into() }).await
    }

    #[tokio::test]
    async fn deny_signed_in()
    {
        assert_eq!(decide(&DenySignedIn, Access::SignedIn).await, AccessDecision::Denied);
        assert_eq!(decide(&DenySignedIn, Access::Public).await, AccessDecision::Allowed(Identity::anonymous()));
    }

    #[tokio::test]
    async fn dev_allow_all()
    {
        assert_eq!(decide(&DevAllowAll, Access::SignedIn).await, AccessDecision::Allowed(Identity::anonymous()));
        assert_eq!(decide(&DevAllowAll, Access::Public).await, AccessDecision::Allowed(Identity::anonymous()));
    }
}
