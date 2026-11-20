//! Values in and out as mysqli gives them: parameters in text format, results decoded from binary by type.

use bytes::BytesMut;
use postgres::types::{Format, IsNull, ToSql, Type};
use speedyspec_data::DbValue;
use speedyspec_data_postgres::__test::{PgCell, PgParam, decode};

fn numeric(weight: i16, negative: bool, dscale: u16, digits: &[i16]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend((digits.len() as i16).to_be_bytes());
    out.extend(weight.to_be_bytes());
    out.extend((if negative { 0x4000u16 } else { 0 }).to_be_bytes());
    out.extend(dscale.to_be_bytes());
    for d in digits {
        out.extend(d.to_be_bytes());
    }
    out
}

fn t(s: &str) -> DbValue {
    DbValue::Text(s.into())
}

#[test]
fn decodes_integers_and_bools() {
    assert_eq!(
        decode(&Type::INT8, &(-5i64).to_be_bytes()).unwrap(),
        DbValue::Int(-5)
    );
    assert_eq!(decode(&Type::BOOL, &[1]).unwrap(), DbValue::Int(1));
}

#[test]
fn decodes_numeric_with_scale() {
    assert_eq!(
        decode(&Type::NUMERIC, &numeric(0, false, 2, &[1, 5000])).unwrap(),
        t("1.50")
    );
    assert_eq!(
        decode(&Type::NUMERIC, &numeric(1, true, 3, &[1, 2345, 6780])).unwrap(),
        t("-12345.678")
    );
    assert_eq!(
        decode(&Type::NUMERIC, &numeric(0, false, 0, &[])).unwrap(),
        t("0")
    );
}

#[test]
fn decodes_timestamps() {
    assert_eq!(
        decode(&Type::TIMESTAMP, &0i64.to_be_bytes()).unwrap(),
        t("2000-01-01 00:00:00")
    );
    assert_eq!(
        decode(&Type::TIMESTAMP, &i64::MIN.to_be_bytes()).unwrap(),
        t("0000-00-00 00:00:00")
    );
    assert_eq!(
        decode(&Type::TIMESTAMP, &i64::MAX.to_be_bytes()).unwrap(),
        t("9999-12-31 23:59:59")
    );
    assert_eq!(
        decode(&Type::TIMESTAMP, &1_500_000i64.to_be_bytes()).unwrap(),
        t("2000-01-01 00:00:01.500000")
    );
    assert_eq!(
        decode(&Type::DATE, &i32::MIN.to_be_bytes()).unwrap(),
        t("0000-00-00")
    );
}

#[test]
fn float_round_trips() {
    assert_eq!(
        decode(&Type::FLOAT8, &0.1f64.to_be_bytes()).unwrap(),
        DbValue::Float(0.1)
    );
}

#[test]
fn unknown_type_unsupported() {
    assert!(matches!(
        decode(&Type::POINT, &[0; 16]),
        Err(speedyspec_data::DbError::Unsupported { .. })
    ));
    let _ = PgCell(DbValue::Null);
}

#[test]
fn params_in_text_format() {
    let mut out = BytesMut::new();
    assert!(matches!(
        PgParam(&DbValue::Int(5))
            .to_sql(&Type::INT8, &mut out)
            .unwrap(),
        IsNull::No
    ));
    assert_eq!(&out[..], b"5");
    let mut out = BytesMut::new();
    assert!(matches!(
        PgParam(&DbValue::Null)
            .to_sql(&Type::TEXT, &mut out)
            .unwrap(),
        IsNull::Yes
    ));
    let mut out = BytesMut::new();
    PgParam(&DbValue::Bytes(vec![0x0a, 0xff]))
        .to_sql(&Type::BYTEA, &mut out)
        .unwrap();
    assert_eq!(&out[..], b"\\x0aff");
    for ty in [Type::INT8, Type::TEXT, Type::BYTEA, Type::TIMESTAMP] {
        assert!(matches!(
            PgParam(&DbValue::Int(1)).encode_format(&ty),
            Format::Text
        ));
    }
}
