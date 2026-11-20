//! The transient functions, native.

use speedyspec_php_ext::Value;
use speedyspec_wp_options::{
    DeleteExpiredTransients, DeleteSiteTransient, DeleteTransient, GetSiteTransient, GetTransient,
    SetSiteTransient, SetTransient, php_empty,
};

use crate::native::{Native, a, ask, h, name, or, send};

fn transient(args: &[Value]) -> Option<String> {
    name(&a(args, 0))
}

pub(crate) fn entries() -> Vec<(&'static str, Native)> {
    vec![
        ("get_transient", |x| {
            let Some(name) = transient(x) else {
                return Ok(Value::Bool(false));
            };
            h(|h| ask(&h.get_transient, GetTransient { name }))?
        }),
        ("set_transient", |x| {
            let Some(name) = transient(x) else {
                return Ok(Value::Bool(false));
            };
            let m = SetTransient {
                name,
                value: a(x, 1),
                expiration: or(x, 2, Value::Int(0)),
            };
            h(|h| send(&h.set_transient, m))?.map(Value::Bool)
        }),
        ("delete_transient", |x| {
            let Some(name) = transient(x) else {
                return Ok(Value::Bool(false));
            };
            h(|h| send(&h.delete_transient, DeleteTransient { name }))?.map(Value::Bool)
        }),
        ("get_site_transient", |x| {
            let Some(name) = transient(x) else {
                return Ok(Value::Bool(false));
            };
            h(|h| ask(&h.get_site_transient, GetSiteTransient { name }))?
        }),
        ("set_site_transient", |x| {
            let Some(name) = transient(x) else {
                return Ok(Value::Bool(false));
            };
            let m = SetSiteTransient {
                name,
                value: a(x, 1),
                expiration: or(x, 2, Value::Int(0)),
            };
            h(|h| send(&h.set_site_transient, m))?.map(Value::Bool)
        }),
        ("delete_site_transient", |x| {
            let Some(name) = transient(x) else {
                return Ok(Value::Bool(false));
            };
            h(|h| send(&h.delete_site_transient, DeleteSiteTransient { name }))?.map(Value::Bool)
        }),
        ("delete_expired_transients", |x| {
            let force_db = !php_empty(&a(x, 0).read());
            h(|h| send(&h.delete_expired, DeleteExpiredTransients { force_db }))??;
            Ok(Value::Null)
        }),
    ]
}
