//! [`GateCalls`] over FastCGI: each call is an internal request to the app's live release, on its internal worker.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http::{HeaderMap, Method};
use speedyspec_server_domain::{AppId, HostMatch, MountTable};
use speedyspec_server_gate::{BoxFuture, CallError, GateCalls};

use super::handler::InternalError;
use super::runtime::PhpRuntime;

/// Where an app's internal requests are addressed: its first mount's path, and that mount's host (`localhost` for
/// `*`).
#[derive(Clone, Debug)]
struct Addressed {
    mount: String,
    host: String,
}

/// Sends the gate's and the health checks' calls to the live php releases.
pub struct FastCgiGateCalls {
    php: Arc<PhpRuntime>,
    addressed: HashMap<AppId, Addressed>,
    timeout: Duration,
}

impl FastCgiGateCalls {
    /// `timeout` bounds every call.
    pub fn new(php: Arc<PhpRuntime>, mounts: &MountTable, timeout: Duration) -> FastCgiGateCalls {
        let mut addressed = HashMap::new();
        for mount in mounts.mounts() {
            addressed
                .entry(mount.app.clone())
                .or_insert_with(|| Addressed {
                    mount: mount.path.clone(),
                    host: match &mount.host {
                        HostMatch::Any => "localhost".to_string(),
                        HostMatch::Exact(host) => host.clone(),
                    },
                });
        }
        FastCgiGateCalls {
            php,
            addressed,
            timeout,
        }
    }

    async fn call(
        &self,
        app: &AppId,
        method: Method,
        path: &str,
        headers: HeaderMap,
        body: Bytes,
        timeout: Duration,
    ) -> Result<http::Response<Bytes>, CallError> {
        let handler = self
            .php
            .live_handler(app)
            .ok_or_else(|| CallError::Unavailable(format!("{app} has no live release")))?;
        let addressed = self.addressed.get(app).cloned().unwrap_or(Addressed {
            mount: "/".to_string(),
            host: "localhost".to_string(),
        });
        handler
            .internal(
                &addressed.mount,
                &addressed.host,
                method,
                path,
                headers,
                body,
                timeout,
            )
            .await
            .map_err(|err| match err {
                InternalError::Timeout => CallError::Timeout,
                InternalError::Unavailable(why) => CallError::Unavailable(why),
            })
    }
}

impl GateCalls for FastCgiGateCalls {
    fn probe<'a>(&'a self, app: &'a AppId, path: &'a str) -> BoxFuture<'a, Result<u16, CallError>> {
        Box::pin(async move {
            let response = self
                .call(
                    app,
                    Method::GET,
                    path,
                    HeaderMap::new(),
                    Bytes::new(),
                    self.timeout,
                )
                .await?;
            Ok(response.status().as_u16())
        })
    }
}
