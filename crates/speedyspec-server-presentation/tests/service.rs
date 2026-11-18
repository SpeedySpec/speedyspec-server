use std::fs;
use std::path::Path;
use std::sync::Arc;

use salvo::http::uri::Scheme;
use salvo::http::{ReqBody, StatusCode};
use salvo::prelude::*;
use salvo::test::ResponseExt;
use speedyspec_server_domain::{
    Access, AccessCheck, AppId, HostMatch, Mount, MountTable, StaticSection,
};
use speedyspec_server_infra_access_dev::{DenySignedIn, DevAllowAll};
use speedyspec_server_presentation::{AppHandle, ForwardedHeaders, ServeState, build_router};
use speedyspec_server_static::StaticSet;

const ADMIN_SECRET: &str = "admin secret page";

fn write(release: &Path, rel: &str, bytes: &[u8]) {
    let path = release.join("dist").join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn set(dir: &Path, files: &[(&str, &str)]) -> Arc<StaticSet> {
    for (rel, body) in files {
        write(dir, rel, body.as_bytes());
    }
    let section = StaticSection {
        root: "dist".to_string(),
        not_found: None,
        cache: Vec::new(),
    };
    Arc::new(StaticSet::build(dir, &section, u64::MAX).unwrap())
}

fn mount(path: &str, app: &str, access: Access) -> Mount {
    Mount {
        host: HostMatch::Any,
        path: path.to_string(),
        app: AppId::parse(app).unwrap(),
        access,
    }
}

fn service(access: Arc<dyn AccessCheck>) -> (tempfile::TempDir, Service) {
    let dir = tempfile::tempdir().unwrap();
    let public = set(
        &dir.path().join("public"),
        &[
            ("index.html", "public index"),
            ("admin/index.html", "not admin"),
        ],
    );
    let admin = set(
        &dir.path().join("admin"),
        &[
            ("index.html", ADMIN_SECRET),
            ("_astro/app.js", ADMIN_SECRET),
        ],
    );
    let mounts = MountTable::new(vec![
        mount("/", "site", Access::Public),
        mount("/admin/", "admin", Access::SignedIn),
        mount("/admin/api/", "server", Access::SignedIn),
        mount("/blog/", "blog", Access::Public),
    ])
    .unwrap();
    let apps = [
        ("site", AppHandle::Static(public)),
        ("admin", AppHandle::Static(admin)),
        ("server", AppHandle::AdminApiPlaceholder),
        ("blog", AppHandle::PhpPlaceholder),
    ]
    .into_iter()
    .map(|(id, handle)| (AppId::parse(id).unwrap(), handle))
    .collect();
    let state = ServeState {
        mounts,
        apps,
        access,
        trusted_proxies: Vec::new(),
        forwarded_headers: ForwardedHeaders::XForwarded,
    };
    (dir, Service::new(build_router(Arc::new(state))))
}

/// Sends a request with the path exactly as given: no URL parser normalizes it first.
async fn raw(service: &Service, path: &str) -> Response {
    let request = http::Request::builder()
        .uri(path)
        .header("host", "example.com")
        .body(ReqBody::None)
        .unwrap();
    service
        .handle(Request::from_hyper(request, Scheme::HTTP))
        .await
}

async fn body(response: &mut Response) -> String {
    response.take_string().await.unwrap()
}

#[tokio::test]
async fn public_static_served() {
    let (_dir, service) = service(Arc::new(DenySignedIn));
    let mut response = raw(&service, "/").await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    assert_eq!(body(&mut response).await, "public index");
}

#[tokio::test]
async fn signed_in_denied() {
    let (_dir, service) = service(Arc::new(DenySignedIn));
    for path in ["/admin/", "/admin/_astro/app.js", "/admin/api/anything"] {
        let mut response = raw(&service, path).await;
        assert_eq!(
            response.status_code,
            Some(StatusCode::UNAUTHORIZED),
            "{path}"
        );
        assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
        let text = body(&mut response).await;
        assert!(!text.contains(ADMIN_SECRET), "{path}: {text}");
        assert_eq!(text, "sign-in required");
    }
}

#[tokio::test]
async fn traversal_cannot_reach_admin_through_public() {
    let (_dir, service) = service(Arc::new(DenySignedIn));
    assert_eq!(
        raw(&service, "/x/../admin/").await.status_code,
        Some(StatusCode::UNAUTHORIZED)
    );
    assert_eq!(
        raw(&service, "/%61dmin/").await.status_code,
        Some(StatusCode::UNAUTHORIZED)
    );
    assert_eq!(
        raw(&service, "//admin/").await.status_code,
        Some(StatusCode::UNAUTHORIZED)
    );
    assert_eq!(
        raw(&service, "/admin%2Findex.html").await.status_code,
        Some(StatusCode::BAD_REQUEST)
    );
    assert_eq!(
        raw(&service, "/a%00b").await.status_code,
        Some(StatusCode::BAD_REQUEST)
    );
    assert_eq!(
        raw(&service, "/../admin/").await.status_code,
        Some(StatusCode::BAD_REQUEST)
    );
}

#[tokio::test]
async fn mount_root_redirect() {
    let (_dir, service) = service(Arc::new(DenySignedIn));
    let response = raw(&service, "/admin?a=1").await;
    assert_eq!(response.status_code, Some(StatusCode::PERMANENT_REDIRECT));
    assert_eq!(response.headers().get("location").unwrap(), "/admin/?a=1");
}

#[tokio::test]
async fn signed_in_served_when_allowed() {
    let (_dir, service) = service(Arc::new(DevAllowAll));
    let mut response = raw(&service, "/admin/").await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    assert_eq!(body(&mut response).await, ADMIN_SECRET);
}

#[tokio::test]
async fn php_placeholder() {
    let (_dir, service) = service(Arc::new(DenySignedIn));
    let response = raw(&service, "/blog/").await;
    assert_eq!(response.status_code, Some(StatusCode::SERVICE_UNAVAILABLE));
    assert_eq!(response.headers().get("retry-after").unwrap(), "30");
}

#[tokio::test]
async fn admin_api_placeholder() {
    let (_dir, service) = service(Arc::new(DevAllowAll));
    assert_eq!(
        raw(&service, "/admin/api/anything").await.status_code,
        Some(StatusCode::NOT_FOUND)
    );
}

#[tokio::test]
async fn head_and_conditional_pass_through() {
    let (_dir, service) = service(Arc::new(DenySignedIn));
    let request = http::Request::builder()
        .method("HEAD")
        .uri("/")
        .header("host", "example.com")
        .body(ReqBody::None)
        .unwrap();
    let mut response = service
        .handle(Request::from_hyper(request, Scheme::HTTP))
        .await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    assert_eq!(response.headers().get("content-length").unwrap(), "12");
    assert_eq!(body(&mut response).await, "");
}

#[tokio::test]
async fn no_mount() {
    let dir = tempfile::tempdir().unwrap();
    let admin = set(dir.path(), &[("index.html", "x")]);
    let mounts = MountTable::new(vec![mount("/admin/", "admin", Access::Public)]).unwrap();
    let apps = [(AppId::parse("admin").unwrap(), AppHandle::Static(admin))]
        .into_iter()
        .collect();
    let state = ServeState {
        mounts,
        apps,
        access: Arc::new(DenySignedIn),
        trusted_proxies: Vec::new(),
        forwarded_headers: ForwardedHeaders::XForwarded,
    };
    let service = Service::new(build_router(Arc::new(state)));
    assert_eq!(
        raw(&service, "/x").await.status_code,
        Some(StatusCode::NOT_FOUND)
    );
}

#[tokio::test]
async fn mount_redirect_location_is_percent_encoded() {
    let dir = tempfile::tempdir().unwrap();
    let menu = set(dir.path(), &[("index.html", "x")]);
    let mounts = MountTable::new(vec![mount("/menü/", "menu", Access::Public)]).unwrap();
    let apps = [(AppId::parse("menu").unwrap(), AppHandle::Static(menu))]
        .into_iter()
        .collect();
    let state = ServeState {
        mounts,
        apps,
        access: Arc::new(DenySignedIn),
        trusted_proxies: Vec::new(),
        forwarded_headers: ForwardedHeaders::XForwarded,
    };
    let service = Service::new(build_router(Arc::new(state)));
    let response = raw(&service, "/men%C3%BC").await;
    assert_eq!(response.status_code, Some(StatusCode::PERMANENT_REDIRECT));
    assert_eq!(response.headers().get("location").unwrap(), "/men%C3%BC/");
}
