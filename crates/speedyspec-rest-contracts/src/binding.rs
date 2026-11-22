//! A REST binding: where an operation is served.

use speedyspec_operation_contracts::Value;

/// An HTTP method a REST route serves. `HEAD` is served as `GET`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RestMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl RestMethod {
    /// The method named `name`, ignoring case.
    pub fn parse(name: &str) -> Option<RestMethod> {
        Some(match name.to_ascii_uppercase().as_str() {
            "GET" => RestMethod::Get,
            "POST" => RestMethod::Post,
            "PUT" => RestMethod::Put,
            "PATCH" => RestMethod::Patch,
            "DELETE" => RestMethod::Delete,
            _ => return None,
        })
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            RestMethod::Get => "GET",
            RestMethod::Post => "POST",
            RestMethod::Put => "PUT",
            RestMethod::Patch => "PATCH",
            RestMethod::Delete => "DELETE",
        }
    }
}

/// Where an operation is served: `/{namespace}{route}` for `methods`. `route` is a WordPress route pattern (a regex with
/// named groups for URL parameters).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RestBinding {
    pub namespace: String,
    pub route: String,
    pub methods: Vec<RestMethod>,
}

fn valid_namespace(namespace: &str) -> bool {
    let Some((vendor, version)) = namespace.split_once('/') else {
        return false;
    };
    !vendor.is_empty()
        && vendor
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && version.len() > 1
        && version.starts_with('v')
        && version[1..].bytes().all(|b| b.is_ascii_digit())
}

impl RestBinding {
    /// Reads `{"namespace", "route", "methods"}`.
    pub fn from_value(value: &Value) -> Result<RestBinding, String> {
        let text = |key: &str| match value.get(key) {
            Some(Value::String(s)) => Ok(s.clone()),
            _ => Err(format!("{key} must be a string")),
        };
        let namespace = text("namespace")?;
        if !valid_namespace(&namespace) {
            return Err("namespace must be vendor/version".into());
        }
        let route = text("route")?;
        if !route.starts_with('/') {
            return Err("route must start with /".into());
        }
        let Some(Value::List(names)) = value.get("methods") else {
            return Err("methods must be a list".into());
        };
        if names.is_empty() {
            return Err("methods must not be empty".into());
        }
        let methods = names
            .iter()
            .map(|m| match m {
                Value::String(name) => {
                    RestMethod::parse(name).ok_or_else(|| format!("unknown method {name}"))
                }
                _ => Err("methods must be strings".into()),
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(RestBinding {
            namespace,
            route,
            methods,
        })
    }

    pub fn to_value(&self) -> Value {
        Value::map([
            ("namespace", Value::from(self.namespace.as_str())),
            ("route", Value::from(self.route.as_str())),
            (
                "methods",
                Value::List(
                    self.methods
                        .iter()
                        .map(|m| Value::from(m.as_str()))
                        .collect(),
                ),
            ),
        ])
    }

    /// `/{namespace}{route}`, as the route table matches it.
    pub fn full_route(&self) -> String {
        format!("/{}{}", self.namespace, self.route)
    }
}
