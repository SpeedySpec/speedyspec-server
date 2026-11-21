use std::rc::Rc;

use speedyspec_firewall::FirewallLog;
use speedyspec_firewall_app::FirewallCache;
use speedyspec_wp_hooks::{Callback, HookApi, HookError, HookName, HookValue};

use crate::stage::CurrentClient;

/// Registers the firewall's Rust callbacks (Observer): `wp_login_failed` counts a failed sign-in for the current
/// client, and `xmlrpc_enabled` turns XML-RPC off when the settings say so.
pub fn register_hooks(
    hooks: &HookApi,
    cache: Rc<FirewallCache>,
    current: CurrentClient,
    log: Rc<dyn FirewallLog>,
) -> Result<(), HookError> {
    let login_cache = cache.clone();
    let failed = Callback::rust(
        "speedyspec_firewall_login_failed",
        move |_: &[HookValue]| {
            if let (Some(guard), Some(client)) = (login_cache.login_guard(), current.get())
                && let Err(error) = guard.record_failure(client)
            {
                log.warn(&format!("login guard: {error}"));
            }
            Ok(HookValue::Null)
        },
    );
    hooks.add_filter(
        &HookName::from("wp_login_failed"),
        failed,
        &HookValue::Int(10),
        1,
    )?;

    let xmlrpc = Callback::rust(
        "speedyspec_firewall_xmlrpc_enabled",
        move |args: &[HookValue]| {
            let blocked = cache
                .settings()
                .is_some_and(|settings| settings.enabled && settings.block_xmlrpc);
            Ok(if blocked {
                HookValue::Bool(false)
            } else {
                args.first().cloned().unwrap_or(HookValue::Null)
            })
        },
    );
    hooks.add_filter(
        &HookName::from("xmlrpc_enabled"),
        xmlrpc,
        &HookValue::Int(99),
        1,
    )?;
    Ok(())
}
