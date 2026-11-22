//! The generic raw contracts behind backward compatibility: legacy callbacks get raw arguments and return raw values.

use indexmap::IndexMap;

use crate::{Category, Extract, Hydrate, OperationError, Value};

/// Raw arguments plus the transport details legacy callbacks read.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct RawInput {
    pub args: Value,
    pub method: Option<String>,
    pub route: Option<String>,
    pub headers: IndexMap<String, String>,
    pub files: IndexMap<String, Value>,
}

/// A raw return value plus any status and headers.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct RawOutput {
    pub value: Value,
    pub status: Option<u16>,
    pub headers: IndexMap<String, String>,
}

fn bad(code: &str, why: &str) -> OperationError {
    OperationError::new(code, Category::InvalidArgument, why)
}

fn optional_text(
    value: Option<&Value>,
    code: &str,
    key: &str,
) -> Result<Option<String>, OperationError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(bad(code, &format!("{key} must be a string"))),
    }
}

fn headers_of(
    value: Option<&Value>,
    code: &str,
) -> Result<IndexMap<String, String>, OperationError> {
    match value {
        None | Some(Value::Null) => Ok(IndexMap::new()),
        Some(Value::Map(map)) => map
            .iter()
            .map(|(k, v)| match v {
                Value::String(s) => Ok((k.clone(), s.clone())),
                _ => Err(bad(code, "header values must be strings")),
            })
            .collect(),
        Some(_) => Err(bad(code, "headers must be a map")),
    }
}

fn headers_value(headers: &IndexMap<String, String>) -> Value {
    Value::Map(
        headers
            .iter()
            .map(|(k, v)| (k.clone(), Value::from(v.as_str())))
            .collect(),
    )
}

fn text_value(text: &Option<String>) -> Value {
    text.as_deref().map_or(Value::Null, Value::from)
}

impl Hydrate for RawInput {
    fn hydrate(value: &Value) -> Result<Self, OperationError> {
        const CODE: &str = "invalid_raw_input";
        if !matches!(value, Value::Map(_)) {
            return Err(bad(CODE, "raw input must be a map"));
        }
        let files = match value.get("files") {
            None | Some(Value::Null) => IndexMap::new(),
            Some(Value::Map(map)) => map.clone(),
            Some(_) => return Err(bad(CODE, "files must be a map")),
        };
        Ok(RawInput {
            args: value.get("args").cloned().unwrap_or(Value::Null),
            method: optional_text(value.get("method"), CODE, "method")?,
            route: optional_text(value.get("route"), CODE, "route")?,
            headers: headers_of(value.get("headers"), CODE)?,
            files,
        })
    }
}

impl Extract for RawInput {
    fn extract(&self) -> Value {
        Value::map([
            ("args", self.args.clone()),
            ("method", text_value(&self.method)),
            ("route", text_value(&self.route)),
            ("headers", headers_value(&self.headers)),
            ("files", Value::Map(self.files.clone())),
        ])
    }
}

impl Hydrate for RawOutput {
    fn hydrate(value: &Value) -> Result<Self, OperationError> {
        const CODE: &str = "invalid_raw_output";
        if !matches!(value, Value::Map(_)) {
            return Err(bad(CODE, "raw output must be a map"));
        }
        let status = match value.get("status") {
            None | Some(Value::Null) => None,
            Some(Value::Int(n)) => {
                Some(u16::try_from(*n).map_err(|_| bad(CODE, "status is out of range"))?)
            }
            Some(_) => return Err(bad(CODE, "status must be an integer")),
        };
        Ok(RawOutput {
            value: value.get("value").cloned().unwrap_or(Value::Null),
            status,
            headers: headers_of(value.get("headers"), CODE)?,
        })
    }
}

impl Extract for RawOutput {
    fn extract(&self) -> Value {
        Value::map([
            ("value", self.value.clone()),
            (
                "status",
                self.status
                    .map_or(Value::Null, |s| Value::Int(i64::from(s))),
            ),
            ("headers", headers_value(&self.headers)),
        ])
    }
}
