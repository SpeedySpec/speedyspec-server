//! The object cache's ports over PHP's WordPress functions, with WordPress's fallbacks when one is absent.

use speedyspec_php::PhpError;
use speedyspec_php_ext::Value;
use speedyspec_php_serialize::{serialize, to_hook_value, unserialize};
use speedyspec_wp_object_cache::{
    AdditionSuspension, CacheNotices, CodecError, SerializeCodec, SiteContext, ValueCodec,
};

pub(crate) fn call(name: &str, args: &[Value]) -> Result<Value, PhpError> {
    speedyspec_php::call(&Value::Str(name.into()), args)
}

pub(crate) fn exists(function: &str) -> bool {
    matches!(
        call("function_exists", &[Value::Str(function.into())]),
        Ok(Value::Bool(true))
    )
}

fn translate(text: &str) -> String {
    if exists("__")
        && let Ok(Value::Str(s)) = call("__", &[Value::Str(text.into())])
    {
        return s;
    }
    text.to_string()
}

/// `is_multisite()` and `get_current_blog_id()` when defined, else a single site, blog 1.
pub struct PhpSiteContext;

impl SiteContext for PhpSiteContext {
    fn is_multisite(&self) -> bool {
        exists("is_multisite") && matches!(call("is_multisite", &[]), Ok(Value::Bool(true)))
    }

    fn current_blog_id(&self) -> i64 {
        if exists("get_current_blog_id")
            && let Ok(Value::Int(id)) = call("get_current_blog_id", &[])
        {
            return id;
        }
        1
    }
}

/// `_doing_it_wrong()` and `_deprecated_function()` when defined; second-level misses go to the log.
pub struct PhpCacheNotices;

impl CacheNotices for PhpCacheNotices {
    fn doing_it_wrong(&self, function: &str, message: &str, version: &str) {
        if exists("_doing_it_wrong") {
            let message = translate(message);
            let _ = call(
                "_doing_it_wrong",
                &[
                    Value::Str(function.into()),
                    Value::Str(message),
                    Value::Str(version.into()),
                ],
            );
        }
    }

    fn deprecated_function(&self, function: &str, version: &str, replacement: &str) {
        if exists("_deprecated_function") {
            let _ = call(
                "_deprecated_function",
                &[
                    Value::Str(function.into()),
                    Value::Str(version.into()),
                    Value::Str(replacement.into()),
                ],
            );
        }
    }

    fn persistence_miss(&self, group: &str, key: &str, why: &str) {
        eprintln!(
            "speedyspec_wordpress: the object cache kept {group}/{key} in the request only: {why}"
        );
    }
}

/// `wp_suspend_cache_addition()` when defined.
pub struct PhpAdditionSuspension;

impl AdditionSuspension for PhpAdditionSuspension {
    fn suspended(&self) -> bool {
        exists("wp_suspend_cache_addition")
            && matches!(
                call("wp_suspend_cache_addition", &[]),
                Ok(Value::Bool(true))
            )
    }
}

/// PHP's serialize format: Rust's own first, PHP's `serialize()` for live PHP objects, PHP's `unserialize()` for
/// stored objects (so they come back as real objects).
pub struct PhpSerializeCodec;

impl ValueCodec for PhpSerializeCodec {
    fn encode(&self, value: &Value) -> Result<Vec<u8>, CodecError> {
        match SerializeCodec.encode(value) {
            Ok(bytes) => Ok(bytes),
            Err(CodecError::Refused(_)) => match speedyspec_php::call_catching(
                &Value::Str("serialize".into()),
                std::slice::from_ref(value),
            ) {
                Ok(Value::Str(s)) => Ok(s.into_bytes()),
                Ok(Value::Bytes(b)) => Ok(b),
                Ok(other) => Err(CodecError::Refused(format!("serialize() gave {other:?}"))),
                Err(failure) => Err(CodecError::Refused(failure.error.to_string())),
            },
            Err(e) => Err(e),
        }
    }

    fn decode(&self, bytes: &[u8]) -> Result<Value, CodecError> {
        let php = unserialize(bytes).map_err(|e| CodecError::Corrupt(e.to_string()))?;
        if !php.contains_object() {
            return Ok(to_hook_value(&php));
        }
        let text = match String::from_utf8(bytes.to_vec()) {
            Ok(s) => Value::Str(s),
            Err(e) => Value::Bytes(e.into_bytes()),
        };
        match call("unserialize", &[text]) {
            Ok(Value::Bool(false)) => Err(CodecError::Corrupt(
                "unserialize() refused the bytes".into(),
            )),
            Ok(v) => Ok(v),
            Err(e) => Err(CodecError::Corrupt(e.to_string())),
        }
    }
}

/// A value going back to PHP: any `OpaquePhpValue` inside (an object Rust kept serialized) becomes a PHP object.
pub(crate) fn materialise(value: Value) -> Value {
    match value {
        Value::Opaque(any) => {
            match any.downcast_ref::<speedyspec_php_serialize::OpaquePhpValue>() {
                Some(opaque) => {
                    let bytes = serialize(&opaque.0);
                    let text = match String::from_utf8(bytes) {
                        Ok(s) => Value::Str(s),
                        Err(e) => Value::Bytes(e.into_bytes()),
                    };
                    call("unserialize", &[text]).unwrap_or(Value::Bool(false))
                }
                None => Value::Opaque(any),
            }
        }
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|(k, v)| (k, materialise(v)))
                .collect(),
        ),
        other => other,
    }
}
