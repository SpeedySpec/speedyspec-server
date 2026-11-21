//! The site and network option functions, native.

use speedyspec_php_ext::Value;
use speedyspec_wp_options::{
    AddNetworkOption, DeleteNetworkOption, GetNetworkOption, LoadCoreSiteOptions,
    PrimeNetworkOptionCaches, UpdateNetworkOption,
};

use crate::native::{Native, R, a, ask, h, name, names, or, send};

fn get(network_id: Value, option: &Value, default: Value) -> R {
    let Some(option) = name(option) else {
        return Ok(Value::Bool(false));
    };
    h(|h| {
        ask(
            &h.get_network,
            GetNetworkOption {
                network_id,
                option,
                default,
            },
        )
    })?
}

fn add(network_id: Value, option: &Value, value: Value) -> R {
    let Some(option) = name(option) else {
        return Ok(Value::Bool(false));
    };
    h(|h| {
        send(
            &h.add_network,
            AddNetworkOption {
                network_id,
                option,
                value,
            },
        )
    })?
    .map(Value::Bool)
}

fn update(network_id: Value, option: &Value, value: Value) -> R {
    let Some(option) = name(option) else {
        return Ok(Value::Bool(false));
    };
    h(|h| {
        send(
            &h.update_network,
            UpdateNetworkOption {
                network_id,
                option,
                value,
            },
        )
    })?
    .map(Value::Bool)
}

fn delete(network_id: Value, option: &Value) -> R {
    let Some(option) = name(option) else {
        return Ok(Value::Bool(false));
    };
    h(|h| {
        send(
            &h.delete_network,
            DeleteNetworkOption { network_id, option },
        )
    })?
    .map(Value::Bool)
}

fn prime(network_id: Value, options: &Value) -> R {
    let options = names(options);
    h(|h| {
        send(
            &h.prime_network,
            PrimeNetworkOptionCaches {
                network_id,
                options,
            },
        )
    })??;
    Ok(Value::Null)
}

pub(crate) fn entries() -> Vec<(&'static str, Native)> {
    vec![
        ("get_site_option", |x| {
            get(Value::Null, &a(x, 0), or(x, 1, Value::Bool(false)))
        }),
        ("add_site_option", |x| add(Value::Null, &a(x, 0), a(x, 1))),
        ("update_site_option", |x| {
            update(Value::Null, &a(x, 0), a(x, 1))
        }),
        ("delete_site_option", |x| delete(Value::Null, &a(x, 0))),
        ("get_network_option", |x| {
            get(a(x, 0), &a(x, 1), or(x, 2, Value::Bool(false)))
        }),
        ("add_network_option", |x| add(a(x, 0), &a(x, 1), a(x, 2))),
        ("update_network_option", |x| {
            update(a(x, 0), &a(x, 1), a(x, 2))
        }),
        ("delete_network_option", |x| delete(a(x, 0), &a(x, 1))),
        ("wp_prime_site_option_caches", |x| {
            prime(Value::Null, &a(x, 0))
        }),
        ("wp_prime_network_option_caches", |x| {
            prime(a(x, 0), &a(x, 1))
        }),
        ("wp_load_core_site_options", |x| {
            let network_id = a(x, 0);
            h(|h| send(&h.load_core, LoadCoreSiteOptions { network_id }))??;
            Ok(Value::Null)
        }),
    ]
}
