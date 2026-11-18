//! The one Salvo handler every request goes through: normalize, find the mount, check access, dispatch.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;

use bytes::Bytes;
use http::header::{CACHE_CONTROL, HOST, LOCATION};
use http::{HeaderValue, StatusCode};
use ipnet::IpNet;
use salvo::http::ResBody;
use salvo::http::uri::Scheme;
use salvo::{Depot, FlowCtrl, Handler, Request, Response, Router, async_trait};
use speedyspec_server_domain::{
    Access, AccessCheck, AccessDecision, AccessRequest, AppId, MountTable,
};

use crate::forwarded::origin;
use crate::handlers::{AppHandle, text};
use crate::path::normalize;

/// Everything a request needs to be answered.
pub struct ServeState {
    pub mounts: MountTable,
    pub apps: HashMap<AppId, AppHandle>,
    pub access: Arc<dyn AccessCheck>,
    pub trusted_proxies: Vec<IpNet>,
}

impl ServeState {
    /// The live handle of an app. Kept behind one function so swappable handles stay a local change.
    pub fn handle(&self, app: &AppId) -> Option<&AppHandle> {
        self.apps.get(app)
    }
}

/// A router that sends every request, whatever its path, through the gate.
pub fn build_router(state: Arc<ServeState>) -> Router {
    let gate = Gate { state };
    Router::new()
        .goal(gate.clone())
        .push(Router::with_path("{**rest}").goal(gate))
}

#[derive(Clone)]
struct Gate {
    state: Arc<ServeState>,
}

#[async_trait]
impl Handler for Gate {
    async fn handle(
        &self,
        req: &mut Request,
        _depot: &mut Depot,
        res: &mut Response,
        ctrl: &mut FlowCtrl,
    ) {
        let answer = self.answer(req).await;
        write(res, answer);
        ctrl.skip_rest();
    }
}

impl Gate {
    async fn answer(&self, req: &Request) -> http::Response<Bytes> {
        let state = &self.state;
        let path = match normalize(req.uri().path()) {
            Ok(path) => path,
            Err(rejection) => {
                tracing::debug!(path = req.uri().path(), reason = %rejection, "rejected request path");
                return text(StatusCode::BAD_REQUEST, "bad request path");
            }
        };

        let peer = req
            .remote_addr()
            .clone()
            .into_std()
            .map_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED), |a| a.ip());
        let tls = *req.scheme() == Scheme::HTTPS;
        let seen = match (req.headers().get(HOST), req.uri().authority()) {
            (None, Some(authority)) => {
                let mut headers = req.headers().clone();
                if let Ok(value) = HeaderValue::from_str(authority.as_str()) {
                    headers.insert(HOST, value);
                }
                origin(peer, &headers, tls, &state.trusted_proxies)
            }
            _ => origin(peer, req.headers(), tls, &state.trusted_proxies),
        };
        let query = req.uri().query();

        if !path.ends_with('/') {
            let with_slash = format!("{path}/");
            if state
                .mounts
                .resolve(&seen.host, &with_slash)
                .is_some_and(|mount| mount.path == with_slash)
            {
                let location = match query {
                    Some(query) => format!("{with_slash}?{query}"),
                    None => with_slash,
                };
                let mut response = text(StatusCode::PERMANENT_REDIRECT, "moved");
                response.headers_mut().insert(
                    LOCATION,
                    HeaderValue::from_str(&location).expect("ASCII after encoding"),
                );
                return response;
            }
        }

        let Some(mount) = state.mounts.resolve(&seen.host, &path) else {
            return text(StatusCode::NOT_FOUND, "not found");
        };
        if mount.access == Access::SignedIn {
            let request = AccessRequest {
                access: mount.access,
                headers: req.headers(),
                client: seen.client,
            };
            if state.access.check(request).await == AccessDecision::Denied {
                let mut response = text(StatusCode::UNAUTHORIZED, "sign-in required");
                response
                    .headers_mut()
                    .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
                return response;
            }
        }
        let Some(handle) = state.handle(&mount.app) else {
            return text(StatusCode::SERVICE_UNAVAILABLE, "service unavailable");
        };
        let rel_path = &path[mount.path.len()..];

        handle.respond(req.method(), &mount.path, rel_path, query, req.headers())
    }
}

fn write(res: &mut Response, answer: http::Response<Bytes>) {
    let (parts, body) = answer.into_parts();
    res.status_code(parts.status);
    for (name, value) in &parts.headers {
        res.headers_mut().insert(name.clone(), value.clone());
    }
    res.body(ResBody::Once(body));
}
