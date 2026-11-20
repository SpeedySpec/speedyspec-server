use std::rc::Rc;

use speedyspec_data::{DbValue, Row, Statement};
use speedyspec_data_query::parse_one;
use speedyspec_wp_db::{LastQuery, MemoryWpdbState, OutputShape, Shaped, WpRow, WpdbState, var};

fn row(columns: &[&str], values: Vec<DbValue>) -> WpRow {
    let columns: Rc<[String]> = columns.iter().map(|c| c.to_string()).collect();
    WpRow::from_row(&Row::new(columns, values))
}

#[test]
fn duplicate_columns_first_position_last_value() {
    let r = row(
        &["id", "name", "id"],
        vec![DbValue::Int(1), DbValue::Text("a".into()), DbValue::Int(2)],
    );
    assert_eq!(
        r.fields,
        vec![
            ("id".to_string(), Some(b"2".to_vec())),
            ("name".to_string(), Some(b"a".to_vec()))
        ]
    );
    match Shaped::of(&[r], OutputShape::ArrayN) {
        Shaped::List(rows) => assert_eq!(rows[0].fields.len(), 2),
        other => panic!("{other:?}"),
    }
}

#[test]
fn object_k_keeps_first() {
    let rows: Vec<WpRow> = [("a", "1"), ("b", "2"), ("a", "3")]
        .iter()
        .map(|(k, v)| {
            row(
                &["k", "v"],
                vec![DbValue::Text(k.to_string()), DbValue::Text(v.to_string())],
            )
        })
        .collect();
    match Shaped::of(&rows, OutputShape::ObjectK) {
        Shaped::Keyed(keyed) => {
            assert_eq!(keyed.len(), 2);
            assert_eq!(keyed[0].0, b"a".to_vec());
            assert_eq!(keyed[0].1, rows[0]);
        }
        other => panic!("{other:?}"),
    }
    // A NULL first column keys as ''.
    let null = vec![row(&["k"], vec![DbValue::Null])];
    assert!(
        matches!(Shaped::of(&null, OutputShape::ObjectK), Shaped::Keyed(k) if k[0].0.is_empty())
    );
}

#[test]
fn get_var_empty_is_null() {
    let rows = vec![WpRow {
        fields: vec![
            ("v".into(), Some(b"".to_vec())),
            ("w".into(), Some(b"0".to_vec())),
            ("n".into(), None),
        ],
    }];
    assert_eq!(var(&rows, 0, 0), None);
    assert_eq!(var(&rows, 1, 0), Some(b"0".to_vec()));
    assert_eq!(var(&rows, 2, 0), None);
    assert_eq!(var(&rows, 5, 0), None);
    assert_eq!(var(&rows, 0, 9), None);
}

#[test]
fn output_shape_parse() {
    for o in ["OBJECT", "object", "Object"] {
        assert_eq!(OutputShape::parse(o), Some(OutputShape::Object));
    }
    assert_eq!(OutputShape::parse("OBJECT_K"), Some(OutputShape::ObjectK));
    assert_eq!(OutputShape::parse("ARRAY_A"), Some(OutputShape::ArrayA));
    assert_eq!(OutputShape::parse("ARRAY_N"), Some(OutputShape::ArrayN));
    assert_eq!(OutputShape::parse("array_a"), None);
    assert_eq!(OutputShape::parse("object_k"), None);
}

#[test]
fn memory_state_round_trip() {
    let state = MemoryWpdbState::new();
    state.modify(&mut |s| s.last_error = "boom".into());
    let mut seen = String::new();
    state.read(&mut |s| seen = s.last_error.clone());
    assert_eq!(seen, "boom");
    state.reset();
    state.read(&mut |s| {
        assert!(s.last_error.is_empty());
        assert!(s.check_current_query);
    });
}

#[test]
fn last_query_renders_params() {
    let (tree, _) = parse_one("SELECT ? , ?").unwrap().into_parts();
    let stmt = Statement::new(tree, vec![DbValue::Text("a'b".into()), DbValue::Int(2)]);
    assert_eq!(
        LastQuery::Statement(Rc::new(stmt)).text(),
        b"SELECT 'a\\'b', 2".to_vec()
    );
    let (tree, _) = parse_one(r"SELECT 'x\\y', ?").unwrap().into_parts();
    let stmt = Statement::new(tree, vec![DbValue::Bytes(vec![0xE9])]);
    assert_eq!(
        LastQuery::Statement(Rc::new(stmt)).text(),
        b"SELECT 'x\\\\y', '\xE9'".to_vec()
    );
    assert_eq!(
        LastQuery::Text(b"SELECT 1".to_vec()).text(),
        b"SELECT 1".to_vec()
    );
}
