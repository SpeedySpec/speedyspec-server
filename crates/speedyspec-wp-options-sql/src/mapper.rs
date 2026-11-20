//! `OptionMapper` (Data Mapper): a `wp_options` row to the option entity and back. `option_id` is the table's.

use speedyspec_data::{DbValue, Row};
use speedyspec_wp_options::Option as OptionEntity;
use speedyspec_wp_options::{Autoload, OptionName, OptionsError, SerializedValue};

pub struct OptionMapper;

fn bytes(value: Option<&DbValue>) -> Option<Vec<u8>> {
    value.and_then(DbValue::to_mysql_text)
}

impl OptionMapper {
    pub fn to_entity(row: &Row) -> Result<OptionEntity, OptionsError> {
        let name = bytes(row.get("option_name"))
            .and_then(|b| String::from_utf8(b).ok())
            .and_then(|n| OptionName::new(&n))
            .ok_or_else(|| OptionsError::Db("a wp_options row has no option_name".into()))?;
        let value = SerializedValue::new(bytes(row.get("option_value")).unwrap_or_default());
        let autoload = Autoload::parse(
            &bytes(row.get("autoload"))
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_default(),
        );
        Ok(OptionEntity {
            name,
            value,
            autoload,
        })
    }

    /// `option_name`, `option_value` and `autoload`, the value as text when it is UTF-8.
    pub fn to_values(option: &OptionEntity) -> Vec<DbValue> {
        vec![
            DbValue::Text(option.name.as_str().to_string()),
            value(&option.value),
            DbValue::Text(option.autoload.as_str().to_string()),
        ]
    }
}

pub(crate) fn value(value: &SerializedValue) -> DbValue {
    match String::from_utf8(value.as_bytes().to_vec()) {
        Ok(text) => DbValue::Text(text),
        Err(e) => DbValue::Bytes(e.into_bytes()),
    }
}
