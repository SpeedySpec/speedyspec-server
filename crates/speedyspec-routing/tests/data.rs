use speedyspec_http::Method;
use speedyspec_routing::{
    DynamicRoute, ROUTE_DATA_VERSION, RouteData, RouteId, RouteName, Rule, StaticRoute,
};

fn sample() -> RouteData {
    RouteData {
        statics: vec![StaticRoute {
            method: Method::Get,
            path: "/robots.txt".into(),
            id: RouteId(0),
        }],
        dynamics: vec![DynamicRoute {
            methods: vec![Method::Get, Method::Other("PURGE".into())],
            regex: "^/users/([0-9]+)$".into(),
            params: vec!["id".into()],
            id: RouteId(1),
        }],
        before_rules: vec![],
        after_rules: vec![Rule {
            pattern: "([^/]+)/?$".into(),
            query: "index.php?name=$matches[1]".into(),
        }],
        names: vec![(RouteName::new("users.show"), RouteId(1))],
    }
}

#[test]
fn route_data_round_trips() {
    let data = sample();
    assert_eq!(RouteData::from_bytes(&data.to_bytes()), Some(data));
}

#[test]
fn route_data_other_version_is_none() {
    let mut bytes = sample().to_bytes();
    assert_eq!(
        u32::from_le_bytes(bytes[0..4].try_into().unwrap()),
        ROUTE_DATA_VERSION
    );
    bytes[0] ^= 0xff;
    assert_eq!(RouteData::from_bytes(&bytes), None);
}

#[test]
fn route_data_garbage_is_none() {
    assert_eq!(RouteData::from_bytes(b"nope"), None);
    let mut bytes = sample().to_bytes();
    bytes.truncate(bytes.len() - 3);
    assert_eq!(RouteData::from_bytes(&bytes), None);
}
