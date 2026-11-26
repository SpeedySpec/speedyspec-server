use speedyspec_data::{DbError, DialectKind};
use speedyspec_wp_db::{DbHost, WpDbConfig};
use speedyspec_wp_db_contracts::WpDbError;

fn host(input: &str) -> Option<(String, Option<u16>, Option<String>, bool)> {
    DbHost::parse(input).map(|h| (h.host, h.port, h.socket, h.is_ipv6))
}

fn some(
    h: &str,
    port: Option<u16>,
    socket: Option<&str>,
    ipv6: bool,
) -> Option<(String, Option<u16>, Option<String>, bool)> {
    Some((h.to_string(), port, socket.map(str::to_string), ipv6))
}

#[test]
fn parse_db_host_matches_wordpress() {
    let sock = Some("/tmp/mysql:with_colon.sock");
    let cases = [
        ("", some("", None, None, false)),
        (":3306", some("", Some(3306), None, false)),
        (
            ":/tmp/mysql.sock",
            some("", None, Some("/tmp/mysql.sock"), false),
        ),
        (":/tmp/mysql:with_colon.sock", some("", None, sock, false)),
        ("127.0.0.1", some("127.0.0.1", None, None, false)),
        (
            "127.0.0.1:port_as_string",
            some("127.0.0.1", None, None, false),
        ),
        ("127.0.0.1:3306", some("127.0.0.1", Some(3306), None, false)),
        (
            "127.0.0.1:3306:/tmp/mysql:with_colon.sock",
            some("127.0.0.1", Some(3306), sock, false),
        ),
        ("example.com", some("example.com", None, None, false)),
        (
            "example.com:port_as_string",
            some("example.com", None, None, false),
        ),
        (
            "example.com:3306",
            some("example.com", Some(3306), None, false),
        ),
        ("localhost", some("localhost", None, None, false)),
        (
            "localhost:port_as_string",
            some("localhost", None, None, false),
        ),
        (
            "localhost:/tmp/mysql.sock",
            some("localhost", None, Some("/tmp/mysql.sock"), false),
        ),
        (
            "localhost:/tmp/mysql:with_colon.sock",
            some("localhost", None, sock, false),
        ),
        (
            "localhost:port_as_string:/tmp/mysql:with_colon.sock",
            some("localhost", None, sock, false),
        ),
        (
            "0000:0000:0000:0000:0000:0000:0000:0001",
            some("0000:0000:0000:0000:0000:0000:0000:0001", None, None, true),
        ),
        ("::1", some("::1", None, None, true)),
        ("[::1]", some("::1", None, None, true)),
        ("[::1]:3306", some("::1", Some(3306), None, true)),
        ("[::1]:port_as_string", some("::1", None, None, true)),
        (
            "[::1]:3306:/tmp/mysql:with_colon.sock",
            some("::1", Some(3306), sock, true),
        ),
        (
            "2001:0db8:0000:0000:0000:ff00:0042:8329",
            some("2001:0db8:0000:0000:0000:ff00:0042:8329", None, None, true),
        ),
        (
            "2001:db8:0:0:0:ff00:42:8329",
            some("2001:db8:0:0:0:ff00:42:8329", None, None, true),
        ),
        (
            "2001:db8::ff00:42:8329",
            some("2001:db8::ff00:42:8329", None, None, true),
        ),
        ("?::", None),
    ];
    for (input, expected) in cases {
        assert_eq!(host(input), expected, "{input:?}");
    }
}

fn config(host: &str) -> WpDbConfig {
    WpDbConfig {
        user: "u".into(),
        password: "p@ss".into(),
        name: "wp".into(),
        host: host.into(),
        charset: None,
        collate: None,
        table_prefix: "wp_".into(),
    }
}

#[test]
fn engine_config_builds_the_url() {
    let socket = config("localhost:/tmp/mysql.sock")
        .engine_config("default")
        .unwrap();
    assert_eq!(socket.name, "default");
    assert_eq!(socket.url, "mysql://u:p%40ss@localhost/wp");
    assert_eq!(
        socket.options.get("socket").map(String::as_str),
        Some("/tmp/mysql.sock")
    );
    let ipv6 = config("[::1]:3307").engine_config("default").unwrap();
    assert_eq!(ipv6.url, "mysql://u:p%40ss@[::1]:3307/wp");
    let mut charset = config("db");
    charset.charset = Some("utf8mb4".into());
    charset.collate = Some("utf8mb4_unicode_520_ci".into());
    let charset = charset.engine_config("default").unwrap();
    assert_eq!(
        charset.options.get("charset").map(String::as_str),
        Some("utf8mb4")
    );
    assert_eq!(
        charset.options.get("collate").map(String::as_str),
        Some("utf8mb4_unicode_520_ci")
    );
    assert!(
        matches!(config("?::").engine_config("default"), Err(WpDbError::Config(m)) if m == "DB_HOST could not be parsed: ?::")
    );
}

#[test]
fn last_error_wording() {
    let engine = |e| WpDbError::Db(e).last_error();
    assert_eq!(
        engine(DbError::Engine {
            engine: "default".into(),
            code: Some("1146".into()),
            message: "Table 'x.t' doesn't exist".into()
        }),
        "Table 'x.t' doesn't exist"
    );
    assert_eq!(
        engine(DbError::Constraint {
            engine: "default".into(),
            message: "Duplicate entry".into()
        }),
        "Duplicate entry"
    );
    assert_eq!(
        engine(DbError::Deadlock {
            engine: "default".into(),
            message: "Deadlock found".into()
        }),
        "Deadlock found"
    );
    assert_eq!(
        engine(DbError::Parse {
            message: "Expected end".into(),
            line: 1,
            column: 8
        }),
        "Could not parse the query: Expected end at line 1, column 8"
    );
    assert_eq!(
        engine(DbError::Unsupported {
            construct: "SET on SELECT".into(),
            dialect: DialectKind::Sqlite
        }),
        "SET on SELECT is not supported on sqlite"
    );
    assert_eq!(WpDbError::InvalidData("bad".into()).last_error(), "bad");
    assert_eq!(WpDbError::ProcessFields("long".into()).last_error(), "long");
    assert_eq!(WpDbError::NotReady.last_error(), "");
    assert_eq!(WpDbError::EmptyQuery.last_error(), "");
}
