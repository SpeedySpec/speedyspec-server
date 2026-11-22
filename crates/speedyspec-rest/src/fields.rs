//! `_fields`: only the requested fields of a response (`rest_filter_response_fields`).

use indexmap::IndexMap;
use speedyspec_operation_contracts::Value;

/// `data` limited to `fields` (dotted paths select nested keys). A list is filtered item by item. `_links` and
/// `_embedded` are kept only when named.
pub fn filter(data: &Value, fields: &[String]) -> Value {
    match data {
        Value::List(items) => Value::List(items.iter().map(|item| filter(item, fields)).collect()),
        Value::Map(_) => {
            let mut out = Value::Map(IndexMap::new());
            for field in fields {
                let path: Vec<&str> = field.split('.').collect();
                copy(data, &mut out, &path);
            }
            out
        }
        other => other.clone(),
    }
}

fn copy(source: &Value, target: &mut Value, path: &[&str]) {
    let (Some((first, rest)), Value::Map(map), Value::Map(out)) =
        (path.split_first(), source, &mut *target)
    else {
        return;
    };
    let Some(value) = map.get(*first) else {
        return;
    };
    if rest.is_empty() {
        out.insert((*first).to_owned(), value.clone());
        return;
    }
    let slot = out
        .entry((*first).to_owned())
        .or_insert_with(|| Value::Map(IndexMap::new()));
    copy(value, slot, rest);
}
