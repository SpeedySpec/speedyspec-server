use speedyspec_router::{Router, UrlError};

fn router() -> Router<&'static str> {
    let mut router = Router::new();
    router
        .get("/users/{id}", "show")
        .name("users.show")
        .where_("id", "[0-9]+");
    router
        .get("/archive/{year}/{month?}", "archive")
        .name("archive");
    router
}

#[test]
fn url_fills_params() {
    assert_eq!(
        router().url("users.show", &[("id", "42")]).unwrap(),
        "/users/42"
    );
}

#[test]
fn url_drops_missing_optional() {
    assert_eq!(
        router().url("archive", &[("year", "2024")]).unwrap(),
        "/archive/2024"
    );
    assert_eq!(
        router()
            .url("archive", &[("year", "2024"), ("month", "05")])
            .unwrap(),
        "/archive/2024/05"
    );
}

#[test]
fn url_appends_unused_as_query() {
    assert_eq!(
        router()
            .url("users.show", &[("id", "42"), ("tab", "a b")])
            .unwrap(),
        "/users/42?tab=a%20b"
    );
}

#[test]
fn url_checks_constraint() {
    assert_eq!(
        router().url("users.show", &[("id", "x")]),
        Err(UrlError::ConstraintFailed {
            param: "id".into(),
            value: "x".into()
        })
    );
}

#[test]
fn url_errors_for_unknown_and_missing() {
    assert_eq!(
        router().url("nope", &[]),
        Err(UrlError::UnknownName("nope".into()))
    );
    assert_eq!(
        router().url("users.show", &[]),
        Err(UrlError::MissingParam("id".into()))
    );
}

#[test]
fn url_encodes_values() {
    let mut router: Router<&str> = Router::new();
    router.get("/tags/{tag}", "tag").name("tag");
    assert_eq!(
        router.url("tag", &[("tag", "café au lait")]).unwrap(),
        "/tags/caf%C3%A9%20au%20lait"
    );
}
