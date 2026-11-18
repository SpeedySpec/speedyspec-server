//! The access-check contract: whether a request may reach a mount.

use std::future::Future;
use std::net::IpAddr;
use std::pin::Pin;

use crate::Access;

/// Who a request comes from, once an access check has let it through.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Identity {
    /// The signed-in user, when there is one.
    pub user_id: Option<u64>,
}

impl Identity {
    pub fn anonymous() -> Identity {
        Identity { user_id: None }
    }
}

/// The outcome of an access check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AccessDecision {
    Allowed(Identity),
    Denied,
}

/// What an access check sees of a request.
#[derive(Clone, Copy, Debug)]
pub struct AccessRequest<'a> {
    /// The mount's policy.
    pub access: Access,
    pub headers: &'a http::HeaderMap,
    /// The client's address, after trusted-proxy headers are applied.
    pub client: IpAddr,
}

/// Decides whether a request may reach a mount. Runs before any handler, static files included.
///
/// The future is boxed so the trait stays object-safe and an implementation can query a session store.
pub trait AccessCheck: Send + Sync {
    fn check<'a>(
        &'a self,
        request: AccessRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = AccessDecision> + Send + 'a>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Always;

    impl AccessCheck for Always {
        fn check<'a>(
            &'a self,
            _request: AccessRequest<'a>,
        ) -> Pin<Box<dyn Future<Output = AccessDecision> + Send + 'a>> {
            Box::pin(async { AccessDecision::Allowed(Identity::anonymous()) })
        }
    }

    #[test]
    fn access_check_is_object_safe() {
        let check: Box<dyn AccessCheck> = Box::new(Always);
        let headers = http::HeaderMap::new();
        let request = AccessRequest {
            access: crate::Access::SignedIn,
            headers: &headers,
            client: [127, 0, 0, 1].into(),
        };
        let _future = check.check(request);
        assert_eq!(Identity::anonymous().user_id, None);
    }
}
