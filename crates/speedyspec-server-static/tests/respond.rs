use std::fs;
use std::path::Path;

use http::{HeaderMap, HeaderValue, Method, Response, StatusCode, header};
use speedyspec_server_domain::StaticSection;
use speedyspec_server_static::StaticSet;

fn write(release: &Path, rel: &str, bytes: &[u8]) {
    let path = release.join("dist").join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn text(len: usize) -> Vec<u8> {
    (0..len).map(|i| b"abcdefghij"[i % 10]).collect()
}

fn build(not_found: Option<&str>) -> (tempfile::TempDir, StaticSet) {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "index.html", b"root index");
    write(dir.path(), "docs/index.html", b"docs index");
    write(dir.path(), "about.html", b"about page");
    write(dir.path(), "404.html", b"custom 404");
    write(dir.path(), "big.txt", &text(10_000));
    write(dir.path(), "logo.png", &text(4096));
    write(dir.path(), "über menü.html", b"umlaut");
    write(dir.path(), "a+b.css", b"plus");
    let section = StaticSection {
        root: "dist".to_string(),
        not_found: not_found.map(str::to_string),
        cache: Vec::new(),
    };
    let set = StaticSet::build(dir.path(), &section, u64::MAX).unwrap();
    (dir, set)
}

fn get(set: &StaticSet, rel: &str, headers: &[(&'static str, &str)]) -> Response<bytes::Bytes> {
    request(set, Method::GET, rel, None, headers)
}

fn request(
    set: &StaticSet,
    method: Method,
    rel: &str,
    query: Option<&str>,
    headers: &[(&'static str, &str)],
) -> Response<bytes::Bytes> {
    let mut map = HeaderMap::new();
    for (name, value) in headers {
        map.append(*name, HeaderValue::from_str(value).unwrap());
    }
    set.respond(&method, "/admin/", rel, query, &map)
}

fn header<'a>(response: &'a Response<bytes::Bytes>, name: header::HeaderName) -> Option<&'a str> {
    response.headers().get(name).map(|v| v.to_str().unwrap())
}

#[test]
fn resolves_exact_index_and_html() {
    let (_dir, set) = build(None);
    assert_eq!(&get(&set, "", &[]).body()[..], b"root index");
    assert_eq!(&get(&set, "docs/", &[]).body()[..], b"docs index");
    assert_eq!(&get(&set, "about", &[]).body()[..], b"about page");
    assert_eq!(&get(&set, "about.html", &[]).body()[..], b"about page");
}

#[test]
fn redirects_directory_without_slash() {
    let (_dir, set) = build(None);
    let response = request(&set, Method::GET, "docs", Some("a=1"), &[]);
    assert_eq!(response.status(), StatusCode::PERMANENT_REDIRECT);
    assert_eq!(
        header(&response, header::LOCATION),
        Some("/admin/docs/?a=1")
    );
}

#[test]
fn not_found_uses_configured_page() {
    let (_dir, set) = build(Some("404.html"));
    let response = get(&set, "missing", &[]);
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(&response.body()[..], b"custom 404");
    assert_eq!(header(&response, header::CACHE_CONTROL), Some("no-cache"));
}

#[test]
fn not_found_plain() {
    let (_dir, set) = build(None);
    let response = get(&set, "missing", &[]);
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(&response.body()[..], b"not found");
    assert!(
        header(&response, header::CONTENT_TYPE)
            .unwrap()
            .starts_with("text/plain")
    );
}

#[test]
fn method_not_allowed() {
    let (_dir, set) = build(None);
    let response = request(&set, Method::POST, "", None, &[]);
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(header(&response, header::ALLOW), Some("GET, HEAD"));
}

#[test]
fn head_has_length_no_body() {
    let (_dir, set) = build(None);
    let response = request(&set, Method::HEAD, "about.html", None, &[]);
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header(&response, header::CONTENT_LENGTH), Some("10"));
    assert!(response.body().is_empty());
}

#[test]
fn file_headers() {
    let (_dir, set) = build(None);
    let response = get(&set, "about.html", &[]);
    assert_eq!(
        header(&response, header::CONTENT_TYPE),
        Some("text/html; charset=utf-8")
    );
    assert_eq!(header(&response, header::CONTENT_LENGTH), Some("10"));
    assert_eq!(header(&response, header::ACCEPT_RANGES), Some("bytes"));
    assert_eq!(header(&response, header::CACHE_CONTROL), Some("no-cache"));
    assert_eq!(
        header(&response, header::ETAG),
        Some(set.file("about.html").unwrap().etag.as_str())
    );
}

#[test]
fn conditional_304() {
    let (_dir, set) = build(None);
    let etag = set.file("big.txt").unwrap().etag.clone();
    let listed = format!("W/\"x\", {etag}");
    let response = get(&set, "big.txt", &[("if-none-match", &listed)]);
    assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
    assert!(response.body().is_empty());
    assert_eq!(header(&response, header::ETAG), Some(etag.as_str()));
    assert_eq!(header(&response, header::VARY), Some("Accept-Encoding"));
    let weak = format!("W/{etag}");
    assert_eq!(
        get(&set, "big.txt", &[("if-none-match", &weak)]).status(),
        StatusCode::NOT_MODIFIED
    );
    assert_eq!(
        get(&set, "big.txt", &[("if-none-match", "*")]).status(),
        StatusCode::NOT_MODIFIED
    );
    assert_eq!(
        get(&set, "big.txt", &[("if-none-match", "\"other\"")]).status(),
        StatusCode::OK
    );
}

#[test]
fn accept_encoding_weights() {
    let (_dir, set) = build(None);
    let file = set.file("big.txt").unwrap();
    assert!(file.br.is_some() && file.gzip.is_some());

    let none = get(&set, "big.txt", &[]);
    assert_eq!(header(&none, header::CONTENT_ENCODING), None);
    assert_eq!(none.body().len(), 10_000);

    let both = get(&set, "big.txt", &[("accept-encoding", "gzip, br")]);
    assert_eq!(header(&both, header::CONTENT_ENCODING), Some("br"));
    assert_eq!(&both.body()[..], &file.br.as_ref().unwrap()[..]);
    assert_eq!(
        header(&both, header::CONTENT_LENGTH).unwrap(),
        file.br.as_ref().unwrap().len().to_string()
    );

    let no_br = get(&set, "big.txt", &[("accept-encoding", "br;q=0, gzip")]);
    assert_eq!(header(&no_br, header::CONTENT_ENCODING), Some("gzip"));

    let weighted = get(
        &set,
        "big.txt",
        &[("accept-encoding", "gzip;q=0.9, br;q=0.4")],
    );
    assert_eq!(header(&weighted, header::CONTENT_ENCODING), Some("gzip"));

    let star = get(&set, "big.txt", &[("accept-encoding", "*;q=0.5")]);
    assert_eq!(header(&star, header::CONTENT_ENCODING), Some("br"));

    let no_identity = get(&set, "about.html", &[("accept-encoding", "identity;q=0")]);
    assert_eq!(no_identity.status(), StatusCode::OK);
    assert_eq!(&no_identity.body()[..], b"about page");

    assert_eq!(header(&none, header::VARY), Some("Accept-Encoding"));
    assert_eq!(header(&get(&set, "logo.png", &[]), header::VARY), None);
}

#[test]
fn range_cases() {
    let (_dir, set) = build(None);
    let full = text(10_000);

    let first = get(
        &set,
        "big.txt",
        &[("range", "bytes=0-99"), ("accept-encoding", "br")],
    );
    assert_eq!(first.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(&first.body()[..], &full[..100]);
    assert_eq!(
        header(&first, header::CONTENT_RANGE),
        Some("bytes 0-99/10000")
    );
    assert_eq!(header(&first, header::CONTENT_LENGTH), Some("100"));
    assert_eq!(header(&first, header::CONTENT_ENCODING), None);

    let suffix = get(&set, "big.txt", &[("range", "bytes=-500")]);
    assert_eq!(suffix.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(&suffix.body()[..], &full[9500..]);
    assert_eq!(
        header(&suffix, header::CONTENT_RANGE),
        Some("bytes 9500-9999/10000")
    );

    let open = get(&set, "big.txt", &[("range", "bytes=9990-")]);
    assert_eq!(&open.body()[..], &full[9990..]);

    let beyond = get(&set, "big.txt", &[("range", "bytes=9999999-")]);
    assert_eq!(beyond.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(
        header(&beyond, header::CONTENT_RANGE),
        Some("bytes */10000")
    );

    let multiple = get(&set, "big.txt", &[("range", "bytes=0-1,4-5")]);
    assert_eq!(multiple.status(), StatusCode::OK);
    assert_eq!(multiple.body().len(), 10_000);

    let malformed = get(&set, "big.txt", &[("range", "lines=0-1")]);
    assert_eq!(malformed.status(), StatusCode::OK);

    let etag = set.file("big.txt").unwrap().etag.clone();
    let matching = get(
        &set,
        "big.txt",
        &[("if-range", &etag), ("range", "bytes=0-9")],
    );
    assert_eq!(matching.status(), StatusCode::PARTIAL_CONTENT);
    let stale = get(
        &set,
        "big.txt",
        &[("if-range", "\"other\""), ("range", "bytes=0-9")],
    );
    assert_eq!(stale.status(), StatusCode::OK);
    assert_eq!(stale.body().len(), 10_000);
}

#[test]
fn serves_encoded_names() {
    let (_dir, set) = build(None);
    assert_eq!(&get(&set, "über menü.html", &[]).body()[..], b"umlaut");
    assert_eq!(&get(&set, "a+b.css", &[]).body()[..], b"plus");
}

#[test]
fn directory_without_index_falls_back_to_html() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "docs.html", b"docs page");
    write(dir.path(), "docs/intro.html", b"intro");
    let section = StaticSection {
        root: "dist".to_string(),
        not_found: None,
        cache: Vec::new(),
    };
    let set = StaticSet::build(dir.path(), &section, u64::MAX).unwrap();
    let response = get(&set, "docs", &[]);
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(&response.body()[..], b"docs page");
}

#[test]
fn redirect_location_is_percent_encoded() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "über menü/index.html", b"x");
    write(dir.path(), "c#/index.html", b"x");
    write(dir.path(), "a\u{1b}b/index.html", b"x");
    let section = StaticSection {
        root: "dist".to_string(),
        not_found: None,
        cache: Vec::new(),
    };
    let set = StaticSet::build(dir.path(), &section, u64::MAX).unwrap();
    let location = |rel: &str| header(&get(&set, rel, &[]), header::LOCATION).map(str::to_string);
    assert_eq!(
        location("über menü").as_deref(),
        Some("/admin/%C3%BCber%20men%C3%BC/")
    );
    assert_eq!(location("c#").as_deref(), Some("/admin/c%23/"));
    assert_eq!(location("a\u{1b}b").as_deref(), Some("/admin/a%1Bb/"));
}

#[test]
fn each_coding_has_its_own_etag() {
    let (_dir, set) = build(None);
    let identity = get(&set, "big.txt", &[]);
    let br = get(&set, "big.txt", &[("accept-encoding", "br")]);
    let gzip = get(&set, "big.txt", &[("accept-encoding", "gzip")]);
    let tags = [
        identity.headers()[header::ETAG].clone(),
        br.headers()[header::ETAG].clone(),
        gzip.headers()[header::ETAG].clone(),
    ];
    assert_ne!(tags[0], tags[1]);
    assert_ne!(tags[0], tags[2]);
    assert_ne!(tags[1], tags[2]);

    let br_tag = tags[1].to_str().unwrap().to_string();
    let revalidated = get(
        &set,
        "big.txt",
        &[("accept-encoding", "br"), ("if-none-match", &br_tag)],
    );
    assert_eq!(revalidated.status(), StatusCode::NOT_MODIFIED);
    assert_eq!(revalidated.headers()[header::ETAG], tags[1]);

    // A range resumed with the brotli body's ETag must not get identity bytes spliced onto it.
    let resumed = get(
        &set,
        "big.txt",
        &[("if-range", &br_tag), ("range", "bytes=0-9")],
    );
    assert_eq!(resumed.status(), StatusCode::OK);
}
