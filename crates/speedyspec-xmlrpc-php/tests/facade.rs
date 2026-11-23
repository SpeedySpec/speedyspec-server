//! `XmlRpc::bind`.

mod support;

use speedyspec_operation_contracts::Value;
use speedyspec_transport_contracts::BindingStore;
use support::*;

fn main() {
    let b = boot();
    load(&b.php, HOOKS);

    load(
        &b.php,
        "SpeedySpec\\XmlRpc\\XmlRpc::bind( 'acme/x' )->method( 'acme.doX' )->params( 'blog_id', 'username', 'password', 'id', '*' );",
    );
    let stored = b.bindings.binding("acme/x", "xmlrpc");
    let names = ["blog_id", "username", "password", "id", "*"];
    check(
        "bind_stores_method_and_params",
        stored
            == Some(Value::map([
                ("method", Value::from("acme.doX")),
                (
                    "params",
                    Value::List(names.iter().map(|n| Value::from(*n)).collect()),
                ),
            ])),
        &stored,
    );

    load(
        &b.php,
        "SpeedySpec\\XmlRpc\\XmlRpc::bind( 'acme/y' )->method( 'acme.doY' );",
    );
    let stored = b.bindings.binding("acme/y", "xmlrpc");
    check(
        "bind_method_alone_keeps_default_params",
        stored == Some(Value::map([("method", Value::from("acme.doY"))])),
        &stored,
    );

    let refused = eval(
        &b.php,
        "try { SpeedySpec\\XmlRpc\\XmlRpc::bind( 'acme/z' )->method( 'acme.z' )->params( 'bad name' ); return 'stored'; }
         catch ( InvalidArgumentException $e ) { return 'refused'; }",
    );
    check(
        "bind_refuses_unknown_param_names",
        matches!(&refused, Ok(speedyspec_php_ext::Value::Str(s)) if s == "refused"),
        &refused,
    );

    let refused = eval(
        &b.php,
        "try { SpeedySpec\\XmlRpc\\XmlRpc::bind( 'acme/w' )->method( 'system.z' ); return 'stored'; }
         catch ( SpeedySpec\\Operation\\RegistrationException $e ) { return 'refused'; }",
    );
    check(
        "bind_refuses_system_names",
        matches!(&refused, Ok(speedyspec_php_ext::Value::Str(s)) if s == "refused")
            && b.bindings.binding("acme/w", "xmlrpc").is_none(),
        &refused,
    );
}
