use speedyspec_hooks::Value;
use speedyspec_wp_db::{CharsetRules, PlaceholderEscape, ServerCaps, StripContext};
use speedyspec_wp_db_contracts::{ColLength, Field, LengthKind};

#[test]
fn caps_from_server_info() {
    let caps = ServerCaps::new("8.4.3");
    assert_eq!(caps.version, "8.4.3");
    assert!(caps.has_cap("utf8mb4_520"));
    assert!(caps.has_cap("UTF8MB4"));
    assert!(caps.has_cap("collation"));
    assert!(caps.has_cap("identifier_placeholders"));
    assert!(!caps.has_cap("nope"));
    assert_eq!(ServerCaps::new("5.5.5-10.11.6-MariaDB").version, "10.11.6");
    assert!(!ServerCaps::new("5.5.60").has_cap("utf8mb4_520"));
    assert!(ServerCaps::new("5.6").has_cap("utf8mb4_520"));
    assert!(!ServerCaps::new("5.0.6").has_cap("set_charset"));
}

#[test]
fn determine_charset_upgrades() {
    let caps = ServerCaps::new("8.4.3");
    assert_eq!(
        CharsetRules::determine_charset("utf8", "utf8_general_ci", &caps),
        ("utf8mb4".into(), "utf8mb4_unicode_520_ci".into())
    );
    assert_eq!(
        CharsetRules::determine_charset("utf8mb4", "", &caps),
        ("utf8mb4".into(), "utf8mb4_unicode_520_ci".into())
    );
    assert_eq!(
        CharsetRules::determine_charset("utf8", "utf8_bin", &caps),
        ("utf8mb4".into(), "utf8mb4_bin".into())
    );
    assert_eq!(
        CharsetRules::determine_charset("utf8mb4", "", &ServerCaps::new("5.5.60")),
        ("utf8mb4".into(), "utf8mb4_unicode_ci".into())
    );
    assert_eq!(
        CharsetRules::determine_charset("latin1", "", &caps),
        ("latin1".into(), "".into())
    );
}

#[test]
fn col_length_table() {
    assert_eq!(
        CharsetRules::col_length("varchar(191)"),
        Some(ColLength {
            kind: LengthKind::Char,
            length: 191
        })
    );
    assert_eq!(
        CharsetRules::col_length("varbinary(16)"),
        Some(ColLength {
            kind: LengthKind::Byte,
            length: 16
        })
    );
    assert_eq!(
        CharsetRules::col_length("tinytext"),
        Some(ColLength {
            kind: LengthKind::Byte,
            length: 255
        })
    );
    assert_eq!(
        CharsetRules::col_length("TEXT"),
        Some(ColLength {
            kind: LengthKind::Byte,
            length: 65535
        })
    );
    assert_eq!(
        CharsetRules::col_length("longtext"),
        Some(ColLength {
            kind: LengthKind::Byte,
            length: 4294967295
        })
    );
    assert_eq!(CharsetRules::col_length("int(11)"), None);
}

fn len(kind: LengthKind, length: u64) -> Option<ColLength> {
    Some(ColLength { kind, length })
}

fn strip_one(charset: &str, value: &[u8], length: Option<ColLength>) -> Vec<u8> {
    let escape = PlaceholderEscape::new(Some("salt"));
    let cx = StripContext {
        escape: &escape,
        connection_charset: "utf8mb4",
    };
    let mut fields = vec![Field {
        name: "f".into(),
        value: Value::Bytes(value.to_vec()),
        format: "%s".into(),
        charset: Some(charset.into()),
        length,
    }];
    CharsetRules::strip_invalid_text(&mut fields, &cx, &mut |_| {
        panic!("no database lookup expected")
    })
    .unwrap();
    match &fields[0].value {
        Value::Str(s) => s.as_bytes().to_vec(),
        Value::Bytes(b) => b.clone(),
        other => panic!("{other:?}"),
    }
}

#[test]
#[allow(clippy::type_complexity)]
fn strip_utf8_in_rust() {
    use LengthKind::{Byte, Char};
    let rep = |s: &str, n| s.repeat(n).into_bytes();
    let cases: Vec<(&str, Vec<u8>, Vec<u8>, Option<ColLength>)> = vec![
        (
            "latin1",
            b"\xf0\x9f\x8e\xb7".to_vec(),
            b"\xf0\x9f\x8e\xb7".to_vec(),
            len(Char, 100),
        ),
        ("latin1", rep("A", 11), rep("A", 10), len(Char, 10)),
        ("latin1", rep("A", 11), rep("A", 10), len(Byte, 10)),
        (
            "ascii",
            b"Hello World".to_vec(),
            b"Hello World".to_vec(),
            len(Char, 100),
        ),
        ("ascii", rep("A", 11), rep("A", 10), len(Char, 10)),
        ("ascii", rep("A", 11), rep("A", 10), len(Byte, 10)),
        (
            "utf8",
            "H€llo\u{1F608}World¢".into(),
            "H€lloWorld¢".into(),
            len(Char, 100),
        ),
        ("utf8", rep("²３", 10), rep("²３", 5), len(Char, 10)),
        ("utf8", rep("²３", 10), "²３²３".into(), len(Byte, 10)),
        ("utf8", rep("３", 11), rep("３", 10), len(Char, 10)),
        ("utf8", rep("３", 11), "３３３".into(), len(Byte, 10)),
        (
            "utf8mb3",
            "H€llo\u{1F608}World¢".into(),
            "H€lloWorld¢".into(),
            len(Char, 100),
        ),
        ("utf8mb3", rep("３", 10), "３３３".into(), len(Byte, 10)),
        (
            "utf8mb4",
            "H€llo\u{1F608}World¢".into(),
            "H€llo\u{1F608}World¢".into(),
            len(Char, 100),
        ),
        (
            "utf8mb4",
            rep("²３𝟜", 10),
            "²３𝟜²３𝟜²３𝟜²".into(),
            len(Char, 10),
        ),
        ("utf8mb4", rep("²３𝟜", 10), "²３𝟜".into(), len(Byte, 10)),
        ("utf8mb4", rep("𝟜", 11), rep("𝟜", 10), len(Char, 10)),
        ("utf8mb4", rep("𝟜", 10), "𝟜𝟜".into(), len(Byte, 10)),
        ("utf8mb4", b"a\xffb".to_vec(), b"ab".to_vec(), None),
    ];
    for (charset, value, expected, length) in cases {
        assert_eq!(
            strip_one(charset, &value, length),
            expected,
            "{charset} {:?}",
            String::from_utf8_lossy(&value)
        );
    }
}

#[test]
fn strip_other_charset_builds_wordpress_sql() {
    let escape = PlaceholderEscape::new(Some("salt"));
    let cx = StripContext {
        escape: &escape,
        connection_charset: "utf8mb4",
    };
    let mut fields = vec![
        Field {
            name: "koi".into(),
            value: Value::Bytes(b"\xfd\xf2\xfd".to_vec()),
            format: "%s".into(),
            charset: Some("koi8r".into()),
            length: len(LengthKind::Char, 2),
        },
        Field {
            name: "big".into(),
            value: Value::Bytes(b"\xa4\x40".to_vec()),
            format: "%s".into(),
            charset: Some("big5".into()),
            length: None,
        },
        Field {
            name: "n".into(),
            value: Value::Int(3),
            format: "%d".into(),
            charset: Some("big5".into()),
            length: None,
        },
    ];
    let mut seen = Vec::new();
    CharsetRules::strip_invalid_text(&mut fields, &cx, &mut |sql| {
        seen.push(escape.remove_bytes(sql));
        Ok(vec![Some(b"\xfd\xf2".to_vec()), Some(b"\xa4\x40".to_vec())])
    })
    .unwrap();
    assert_eq!(
        seen,
        vec![b"SELECT CONVERT( LEFT( CONVERT( '\xfd\xf2\xfd' USING koi8r ), 2 ) USING utf8mb4 ) AS x_koi, CONVERT( CONVERT( '\xa4\x40' USING big5 ) USING utf8mb4 ) AS x_big".to_vec()]
    );
    assert_eq!(fields[0].value, Value::Bytes(b"\xfd\xf2".to_vec()));
    assert_eq!(fields[2].value, Value::Int(3));
}

#[test]
fn strip_from_query() {
    let escape = PlaceholderEscape::new(Some("salt"));
    let cx = StripContext {
        escape: &escape,
        connection_charset: "utf8mb4",
    };
    let none =
        &mut |_: &[u8]| -> Result<Vec<Option<Vec<u8>>>, speedyspec_wp_db_contracts::WpDbError> {
            panic!("no lookup")
        };
    assert_eq!(
        CharsetRules::strip_invalid_text_from_query(b"SHOW TABLES \xff", "latin1", &cx, none)
            .unwrap(),
        b"SHOW TABLES \xff".to_vec()
    );
    assert_eq!(
        CharsetRules::strip_invalid_text_from_query(
            "INSERT INTO t VALUES ('\u{1F600}')".as_bytes(),
            "utf8",
            &cx,
            none
        )
        .unwrap(),
        b"INSERT INTO t VALUES ('')".to_vec()
    );
    assert_eq!(
        CharsetRules::strip_invalid_text_from_query(
            "SELECT '\u{1F600}'".as_bytes(),
            "utf8mb4",
            &cx,
            none
        )
        .unwrap(),
        "SELECT '\u{1F600}'".as_bytes().to_vec()
    );
}

#[test]
fn check_ascii_cases() {
    assert!(CharsetRules::check_ascii(b"abc"));
    assert!(!CharsetRules::check_ascii("é".as_bytes()));
}
