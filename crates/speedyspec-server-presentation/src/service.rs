//! The one Salvo handler every request goes through: normalize, find the mount, check access, dispatch.

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

use std::collections::HashMap;

use speedyspec_server_gate::{MaintenanceSet, is_internal};

use crate::forwarded::{ForwardedHeaders, origin};
use crate::handlers::{AppHandle, PhpCall, PhpEndpoint, text};
use crate::live::LiveApps;
use crate::path::normalize;
use speedyspec_server_static::encode_path;

/// Everything a request needs to be answered.
pub struct ServeState {
    pub mounts: MountTable,
    /// Shared with the release runtimes, which swap handles when a release goes live.
    pub apps: Arc<LiveApps>,
    pub access: Arc<dyn AccessCheck>,
    pub trusted_proxies: Vec<IpNet>,
    /// Which proxy headers trusted proxies set.
    pub forwarded_headers: ForwardedHeaders,
    /// Every app's maintenance sources.
    pub maintenance: Arc<MaintenanceSet>,
    /// The static app whose maintenance page each app shows.
    pub maintenance_pages: HashMap<AppId, AppId>,
}

impl ServeState {
    /// The live handle of an app. Kept behind one function so swappable handles stay a local change.
    pub fn handle(&self, app: &AppId) -> Option<Arc<AppHandle>> {
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
        match self.answer(req).await {
            Answer::Done(answer) => write(res, answer),
            Answer::Php(endpoint, call) => endpoint.serve(call, req, res).await,
        }
        ctrl.skip_rest();
    }
}

/// What the gate decided.
enum Answer {
    Done(http::Response<Bytes>),
    Php(Arc<dyn PhpEndpoint>, PhpCall),
}

impl From<http::Response<Bytes>> for Answer {
    fn from(response: http::Response<Bytes>) -> Answer {
        Answer::Done(response)
    }
}

impl Gate {
    async fn answer(&self, req: &Request) -> Answer {
        let state = &self.state;
        let path = match normalize(req.uri().path()) {
            Ok(path) => path,
            Err(rejection) => {
                tracing::debug!(path = req.uri().path(), reason = %rejection, "rejected request path");
                return text(StatusCode::BAD_REQUEST, "bad request path").into();
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
                origin(
                    peer,
                    &headers,
                    tls,
                    &state.trusted_proxies,
                    state.forwarded_headers,
                )
            }
            _ => origin(
                peer,
                req.headers(),
                tls,
                &state.trusted_proxies,
                state.forwarded_headers,
            ),
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
                    Some(query) => format!("{}?{query}", encode_path(&with_slash)),
                    None => encode_path(&with_slash),
                };
                let mut response = text(StatusCode::PERMANENT_REDIRECT, "moved");
                response.headers_mut().insert(
                    LOCATION,
                    HeaderValue::from_str(&location)
                        .expect("percent-encoded path and a query from a valid URI"),
                );
                return response.into();
            }
        }

        let Some(mount) = state.mounts.resolve(&seen.host, &path) else {
            return text(StatusCode::NOT_FOUND, "not found").into();
        };
        let rel_path = &path[mount.path.len()..];
        // The server's own paths are never a visitor's, whether or not the visitor could sign in.
        if is_internal(rel_path, &state.apps.internal_paths(&mount.app)) {
            return text(StatusCode::NOT_FOUND, "not found").into();
        }
        // Maintenance comes before sign-in: everyone gets it, and its page needs nothing else.
        if state.maintenance.is_on(&mount.app) {
            return maintenance_answer(state, &mount.app).into();
        }
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
                return response.into();
            }
        }
        let Some(handle) = state.handle(&mount.app) else {
            return text(StatusCode::SERVICE_UNAVAILABLE, "service unavailable").into();
        };

        if let AppHandle::Php(endpoint) = &*handle {
            let call = PhpCall {
                mount_prefix: mount.path.clone(),
                rel_path: rel_path.to_string(),
                origin: seen,
            };
            return Answer::Php(endpoint.clone(), call);
        }
        handle
            .respond(req.method(), &mount.path, rel_path, query, req.headers())
            .into()
    }
}

/// What an app in maintenance answers: its maintenance page from memory, or a small JSON body, with 503.
fn maintenance_answer(state: &ServeState, app: &AppId) -> http::Response<Bytes> {
    let page = state
        .maintenance_pages
        .get(app)
        .and_then(|page_app| state.handle(page_app))
        .and_then(|handle| match &*handle {
            AppHandle::Static(static_app) => static_app.current.maintenance_page().cloned(),
            _ => None,
        });
    let (body, content_type) = match page {
        Some(page) => (page, "text/html; charset=utf-8"),
        None => (
            Bytes::from_static(br#"{"error":"maintenance"}"#),
            "application/json",
        ),
    };
    let mut response = http::Response::new(body);
    *response.status_mut() = StatusCode::SERVICE_UNAVAILABLE;
    let headers = response.headers_mut();
    headers.insert(
        http::header::CONTENT_TYPE,
        HeaderValue::from_static(content_type),
    );
    headers.insert(
        http::header::RETRY_AFTER,
        HeaderValue::from_static(crate::handlers::RETRY_AFTER_SECONDS),
    );
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn write(res: &mut Response, answer: http::Response<Bytes>) {
    let (parts, body) = answer.into_parts();
    res.status_code(parts.status);
    for (name, value) in &parts.headers {
        res.headers_mut().insert(name.clone(), value.clone());
    }
    res.body(ResBody::Once(body));
}
