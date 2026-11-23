//! Where an operation is served over XML-RPC.

use speedyspec_operation_contracts::Value;

/// One positional parameter of an XML-RPC method.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum XmlRpcParam {
    /// The blog ID WordPress's methods take first; read and ignored, as single-site WordPress does.
    BlogId,
    Username,
    Password,
    /// An input property.
    Field(String),
    /// One struct holding every input property no other parameter names.
    Rest,
}

impl XmlRpcParam {
    /// The parameter's binding name: `blog_id`, `username`, `password`, the property, or `*`.
    pub fn name(&self) -> &str {
        match self {
            XmlRpcParam::BlogId => "blog_id",
            XmlRpcParam::Username => "username",
            XmlRpcParam::Password => "password",
            XmlRpcParam::Field(name) => name,
            XmlRpcParam::Rest => "*",
        }
    }

    /// The parameter a binding name stands for.
    pub fn parse(name: &str) -> XmlRpcParam {
        match name {
            "blog_id" => XmlRpcParam::BlogId,
            "username" => XmlRpcParam::Username,
            "password" => XmlRpcParam::Password,
            "*" => XmlRpcParam::Rest,
            field => XmlRpcParam::Field(field.to_owned()),
        }
    }
}

/// An operation's XML-RPC method: its name and positional parameters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XmlRpcBinding {
    pub method: String,
    pub params: Vec<XmlRpcParam>,
}

/// Whether `name` is a dotted method name (`acme.editReport`) outside the reserved `system.` namespace.
fn valid_method(name: &str) -> bool {
    let parts: Vec<&str> = name.split('.').collect();
    parts.len() >= 2
        && parts[0] != "system"
        && parts.iter().all(|part| {
            !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
}

impl XmlRpcBinding {
    /// The binding as the binding store keeps it: `{method, params: [names]}`.
    pub fn to_value(&self) -> Value {
        Value::map([
            ("method", Value::String(self.method.clone())),
            (
                "params",
                Value::List(
                    self.params
                        .iter()
                        .map(|p| Value::String(p.name().to_owned()))
                        .collect(),
                ),
            ),
        ])
    }

    /// The binding a stored value holds; `Err` says what is wrong with it.
    pub fn from_value(value: &Value) -> Result<XmlRpcBinding, String> {
        let Some(Value::String(method)) = value.get("method") else {
            return Err("an XML-RPC binding needs a method name".into());
        };
        if !valid_method(method) {
            return Err(format!(
                "{method} is not a method name: use dotted words (acme.editReport), outside system."
            ));
        }
        let params = match value.get("params") {
            None => Vec::new(),
            Some(Value::List(items)) => items
                .iter()
                .map(|item| match item {
                    Value::String(name) if !name.is_empty() => Ok(XmlRpcParam::parse(name)),
                    _ => Err("XML-RPC parameters are names".to_owned()),
                })
                .collect::<Result<_, _>>()?,
            Some(_) => return Err("XML-RPC parameters are a list of names".into()),
        };
        Ok(XmlRpcBinding {
            method: method.clone(),
            params,
        })
    }
}
