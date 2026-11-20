use std::rc::Rc;

use speedyspec_data::{DbValue, Row};
use speedyspec_wp_options::Option as OptionEntity;
use speedyspec_wp_options::{Autoload, OptionName, SerializedValue};
use speedyspec_wp_options_sql::OptionMapper;

fn row(value: DbValue) -> Row {
    let columns: Rc<[String]> = ["option_id", "option_name", "option_value", "autoload"]
        .iter()
        .map(|c| c.to_string())
        .collect();
    Row::new(
        columns,
        vec![
            DbValue::UInt(7),
            DbValue::Text("blogname".into()),
            value,
            DbValue::Text("on".into()),
        ],
    )
}

#[test]
fn maps_rows_both_ways() {
    let entity = OptionMapper::to_entity(&row(DbValue::Text("My Blog".into()))).unwrap();
    assert_eq!(
        entity,
        OptionEntity {
            name: OptionName::new("blogname").unwrap(),
            value: SerializedValue::new(b"My Blog".to_vec()),
            autoload: Autoload::On,
        }
    );
    assert_eq!(
        OptionMapper::to_values(&entity),
        vec![
            DbValue::Text("blogname".into()),
            DbValue::Text("My Blog".into()),
            DbValue::Text("on".into())
        ]
    );
    let binary = OptionMapper::to_entity(&row(DbValue::Bytes(vec![0x61, 0xff]))).unwrap();
    assert_eq!(binary.value.as_bytes(), &[0x61, 0xff]);
    assert_eq!(
        OptionMapper::to_values(&binary)[1],
        DbValue::Bytes(vec![0x61, 0xff])
    );
    // A row without a name is not an option.
    let columns: Rc<[String]> = ["option_value"].iter().map(|c| c.to_string()).collect();
    assert!(OptionMapper::to_entity(&Row::new(columns, vec![DbValue::Text("x".into())])).is_err());
}
