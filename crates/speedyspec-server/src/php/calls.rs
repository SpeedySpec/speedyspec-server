//! [`GateCalls`] over FastCGI: each call is an internal request to the app's live release, on its internal worker.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http::{HeaderMap, Method};
use speedyspec_server_domain::{AppId, HostMatch, MountTable};
use speedyspec_server_gate::{
    BoxFuture, CallError, ChangeSet, Changes, GateCalls, SessionAnswer, SiteState,
};

use super::handler::InternalError;
use super::pool::InternalLane;
use super::runtime::PhpRuntime;

/// Where an app's internal requests are addressed: its first mount's path, and that mount's host (`localhost` for
/// `*`).
#[derive(Clone, Debug)]
pub(crate) struct Addressed {
    mount: String,
    host: String,
}

/// Each mounted app's address for internal requests.
fn address_apps(mounts: &MountTable) -> HashMap<AppId, Addressed> {
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
    addressed
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
        let addressed = address_apps(mounts);
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
        lane: InternalLane,
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
                lane,
            )
            .await
            .map_err(|err| match err {
                InternalError::Timeout => CallError::Timeout,
                InternalError::Unavailable(why) => CallError::Unavailable(why),
            })
    }
}

impl super::cron::CronCalls for FastCgiGateCalls {
    fn call<'a>(
        &'a self,
        app: &'a AppId,
        path: &'a str,
        query: Option<&'a str>,
        budget: Duration,
        timeout: Duration,
    ) -> BoxFuture<'a, Result<Vec<String>, CallError>> {
        Box::pin(async move {
            let uri = cron_uri(path, budget, query);
            let response = FastCgiGateCalls::call(
                self,
                app,
                Method::GET,
                &uri,
                HeaderMap::new(),
                Bytes::new(),
                timeout,
                InternalLane::Cron,
            )
            .await?;
            if response.status().is_success() {
                Ok(read_follow_ups(response.body()))
            } else {
                Err(CallError::Unavailable(format!(
                    "the cron path answered {}",
                    response.status()
                )))
            }
        })
    }
}

/// The cron call's URI: the declared path with `budget` added (after `&` when the path has a query already), then a
/// follow-up's query.
fn cron_uri(path: &str, budget: Duration, query: Option<&str>) -> String {
    let separator = if path.contains('?') { '&' } else { '?' };
    let mut uri = format!("{path}{separator}budget={}", budget.as_secs_f64());
    if let Some(query) = query {
        uri.push('&');
        uri.push_str(query);
    }
    uri
}

/// The most follow-up calls one answer may list.
const FOLLOW_UPS_MAX: usize = 10_000;

/// The follow-up queries a cron answer lists: the strings of a JSON object's `calls` array. Anything else is none; an
/// entry with `#` or a line break is dropped (it would end or split the URI), and at most [`FOLLOW_UPS_MAX`] are kept.
fn read_follow_ups(body: &[u8]) -> Vec<String> {
    let Ok(serde_json::Value::Object(answer)) = serde_json::from_slice::<serde_json::Value>(body)
    else {
        return Vec::new();
    };
    let Some(serde_json::Value::Array(calls)) = answer.get("calls") else {
        return Vec::new();
    };
    calls
        .iter()
        .filter_map(|call| call.as_str())
        .filter(|query| !query.contains(['#', '\r', '\n']))
        .take(FOLLOW_UPS_MAX)
        .map(str::to_string)
        .collect()
}

/// The most a session answer may be.
const SESSION_MAX_BYTES: usize = 256 * 1024;
/// More pages than this in one answer is not trusted: the user gets none.
const PAGES_MAX: usize = 2000;
/// The longest page id kept.
const PAGE_ID_MAX: usize = 128;

/// A page id as the admin site names its files: two segments of lowercase letters, digits and hyphens, neither
/// starting with a hyphen.
fn page_id(id: &str) -> bool {
    let segment = |s: &str| {
        !s.is_empty()
            && !s.starts_with('-')
            && s.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    };
    id.len() <= PAGE_ID_MAX
        && matches!(id.split_once('/'), Some((a, b)) if segment(a) && segment(b))
}

/// Reads the gate app's session answer. Signed in only for a 200 whose body is a JSON object naming the user by a
/// positive integer id; anything else is signed out. A valid user whose `pages` is not a list of page ids gets no
/// pages, so page files fail closed.
fn read_session(response: &http::Response<Bytes>) -> SessionAnswer {
    if response.status() != http::StatusCode::OK || response.body().len() > SESSION_MAX_BYTES {
        return SessionAnswer::SignedOut;
    }
    let Ok(body) = serde_json::from_slice::<serde_json::Value>(response.body()) else {
        return SessionAnswer::SignedOut;
    };
    let Some(user) = body["user"]["id"].as_u64().filter(|id| *id > 0) else {
        return SessionAnswer::SignedOut;
    };
    let pages = match body["pages"].as_array() {
        Some(list) if list.len() <= PAGES_MAX && list.iter().all(serde_json::Value::is_string) => {
            list.iter()
                .filter_map(serde_json::Value::as_str)
                .filter(|id| page_id(id))
                .map(str::to_string)
                .collect()
        }
        _ => Default::default(),
    };
    SessionAnswer::SignedIn { user, pages }
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
                    InternalLane::Probe,
                )
                .await?;
            Ok(response.status().as_u16())
        })
    }

    fn site<'a>(
        &'a self,
        app: &'a AppId,
        path: &'a str,
    ) -> BoxFuture<'a, Result<SiteState, CallError>> {
        Box::pin(async move {
            let response = self
                .call(
                    app,
                    Method::GET,
                    path,
                    HeaderMap::new(),
                    Bytes::new(),
                    self.timeout,
                    InternalLane::Calls,
                )
                .await?;
            if response.status() != http::StatusCode::OK {
                return Err(CallError::Malformed(format!(
                    "site state answered {}",
                    response.status()
                )));
            }
            let body: serde_json::Value = serde_json::from_slice(response.body())
                .map_err(|err| CallError::Malformed(format!("site state: {err}")))?;
            match (body["installed"].as_bool(), body["maintenance"].as_bool()) {
                (Some(installed), Some(maintenance)) => Ok(SiteState {
                    installed,
                    maintenance,
                }),
                _ => Err(CallError::Malformed(
                    "site state needs boolean installed and maintenance".into(),
                )),
            }
        })
    }

    fn session<'a>(
        &'a self,
        app: &'a AppId,
        path: &'a str,
        cookie: &'a http::HeaderValue,
    ) -> BoxFuture<'a, Result<SessionAnswer, CallError>> {
        Box::pin(async move {
            // Only the visitor's cookie: never their Origin, Referer, nonce or Authorization.
            let mut headers = HeaderMap::new();
            headers.insert(http::header::COOKIE, cookie.clone());
            let response = self
                .call(
                    app,
                    Method::GET,
                    path,
                    headers,
                    Bytes::new(),
                    self.timeout,
                    InternalLane::Calls,
                )
                .await?;
            Ok(read_session(&response))
        })
    }

    fn changes<'a>(
        &'a self,
        app: &'a AppId,
        path: &'a str,
        since: &'a str,
    ) -> BoxFuture<'a, Result<Changes, CallError>> {
        Box::pin(async move {
            let encoded: String = url_encode(since);
            let path = format!("{path}?since={encoded}");
            let response = self
                .call(
                    app,
                    Method::GET,
                    &path,
                    HeaderMap::new(),
                    Bytes::new(),
                    self.timeout,
                    InternalLane::Calls,
                )
                .await?;
            if response.status() != http::StatusCode::OK {
                return Err(CallError::Malformed(format!(
                    "changes answered {}",
                    response.status()
                )));
            }
            read_changes(response.body())
        })
    }
}

/// Percent-encodes a cursor for a query string.
fn url_encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

/// Reads the revocation feed: `{"cursor": "...", "users": [ids]}` or `{"cursor": "...", "reset": true}`.
fn read_changes(body: &[u8]) -> Result<Changes, CallError> {
    let value: serde_json::Value = serde_json::from_slice(body)
        .map_err(|err| CallError::Malformed(format!("changes: {err}")))?;
    let cursor = value["cursor"]
        .as_str()
        .ok_or_else(|| CallError::Malformed("changes needs a string cursor".into()))?
        .to_string();
    if value["reset"].as_bool() == Some(true) {
        return Ok(Changes {
            cursor,
            change: ChangeSet::Reset,
        });
    }
    let users = value["users"]
        .as_array()
        .and_then(|list| {
            list.iter()
                .map(serde_json::Value::as_u64)
                .collect::<Option<Vec<u64>>>()
        })
        .ok_or_else(|| {
            CallError::Malformed("changes needs users, a list of ids, or reset".into())
        })?;
    Ok(Changes {
        cursor,
        change: ChangeSet::Users(users),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(status: u16, body: &str) -> http::Response<Bytes> {
        let mut response = http::Response::new(Bytes::from(body.to_string()));
        *response.status_mut() = http::StatusCode::from_u16(status).unwrap();
        response
    }

    #[test]
    fn read_follow_ups_reads_calls() {
        assert_eq!(
            read_follow_ups(br#"{"calls":["site=2&host=a.test&path=%2F","site=3"],"ran":1}"#),
            vec!["site=2&host=a.test&path=%2F", "site=3"]
        );
        assert!(read_follow_ups(b"").is_empty());
        assert!(read_follow_ups(b"not json").is_empty());
        assert!(read_follow_ups(br#"{}"#).is_empty());
        assert!(read_follow_ups(br#"["site=2"]"#).is_empty());
        assert!(read_follow_ups(br#"{"calls":"site=2"}"#).is_empty());
        assert_eq!(
            read_follow_ups(br#"{"calls":[2,null,"a#b","c\nd","e\rf","ok"]}"#),
            vec!["ok"]
        );
        let many = format!(
            "{{\"calls\":[{}]}}",
            vec!["\"s=1\""; FOLLOW_UPS_MAX + 5].join(",")
        );
        assert_eq!(read_follow_ups(many.as_bytes()).len(), FOLLOW_UPS_MAX);
    }

    #[test]
    fn path_with_query_appends() {
        let budget = Duration::from_secs(30);
        assert_eq!(cron_uri("/cron.php", budget, None), "/cron.php?budget=30");
        assert_eq!(cron_uri("/x?a=1", budget, None), "/x?a=1&budget=30");
        assert_eq!(
            cron_uri("/cron.php", budget, Some("site=2&host=a.test")),
            "/cron.php?budget=30&site=2&host=a.test"
        );
    }

    #[test]
    fn session_answers_fail_closed() {
        let signed_in = read_session(&answer(
            200,
            r#"{"user":{"id":5},"pages":["core/general","Bad/x","a/b/c"]}"#,
        ));
        assert_eq!(
            signed_in,
            SessionAnswer::SignedIn {
                user: 5,
                pages: ["core/general".to_string()].into()
            }
        );
        for (status, body) in [
            (401, r#"{"user":{"id":5}}"#),
            (200, r#"{"user":{"id":0}}"#),
            (200, r#"{"user":{"id":"5"}}"#),
            (200, "not json"),
        ] {
            assert_eq!(
                read_session(&answer(status, body)),
                SessionAnswer::SignedOut,
                "{status} {body}"
            );
        }
        assert_eq!(
            read_session(&answer(200, r#"{"user":{"id":9},"pages":"all"}"#)),
            SessionAnswer::SignedIn {
                user: 9,
                pages: Default::default()
            },
            "a user with a malformed page list gets no pages"
        );
    }
}

/// Runs a release's migration: an internal `POST` to its prepared (not yet live) release, on that release's
/// internal worker. 2xx is success.
pub struct PhpMigrator {
    addressed: HashMap<AppId, Addressed>,
}

impl PhpMigrator {
    pub fn new(mounts: &MountTable) -> PhpMigrator {
        PhpMigrator {
            addressed: address_apps(mounts),
        }
    }
}

impl speedyspec_server_app::Migrator for PhpMigrator {
    fn migrate(
        &self,
        prepared: &speedyspec_server_domain::Prepared,
        path: &str,
        timeout: Duration,
    ) -> Result<(), String> {
        let ready = prepared
            .payload
            .downcast_ref::<super::runtime::Ready>()
            .ok_or("the release is not a prepared php release")?;
        let addressed = self
            .addressed
            .get(&prepared.app)
            .cloned()
            .unwrap_or(Addressed {
                mount: "/".to_string(),
                host: "localhost".to_string(),
            });
        // The release manager runs on blocking threads: the call gets a runtime of its own.
        let answer = std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|err| err.to_string())?;
                    runtime
                        .block_on(ready.handler.internal(
                            &addressed.mount,
                            &addressed.host,
                            Method::POST,
                            path,
                            HeaderMap::new(),
                            Bytes::new(),
                            timeout,
                            InternalLane::Calls,
                        ))
                        .map_err(|err| match err {
                            InternalError::Timeout => {
                                format!("timed out after {}", humantime::format_duration(timeout))
                            }
                            InternalError::Unavailable(why) => why,
                        })
                })
                .join()
                .map_err(|_| "the migration call panicked".to_string())?
        })?;
        if answer.status().is_success() {
            return Ok(());
        }
        let body =
            String::from_utf8_lossy(&answer.body()[..answer.body().len().min(200)]).into_owned();
        Err(format!("{}: {}", answer.status().as_u16(), body.trim()))
    }
}

/// Upgrade maintenance, here at once.
pub struct UpgradeMaintenance(pub Arc<speedyspec_server_gate::MaintenanceSet>);

impl speedyspec_server_app::MaintenanceSink for UpgradeMaintenance {
    fn upgrade(&self, apps: &[AppId], on: bool) {
        for app in apps {
            self.0
                .set(app, speedyspec_server_gate::MaintenanceSource::Upgrade, on);
        }
    }
}
