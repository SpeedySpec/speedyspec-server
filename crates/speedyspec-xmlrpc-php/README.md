# speedyspec-xmlrpc-php

The XML-RPC transport's PHP adapter: WordPress's XML-RPC hooks and login, `XmlRpc::bind`, and the `wp_xmlrpc_server`
shim, whose whole method table is served by the Rust XML-RPC server.

## Binding methods

```php
use SpeedySpec\XmlRpc\XmlRpc;

final class Acme_XmlRpc {
    public static function bind(): void {
        XmlRpc::bind( 'acme/update-report' )->method( 'acme.updateReport' );
        XmlRpc::bind( 'acme/hello' )->method( 'acme.hello' )->params( 'name' );
    }
}
add_action( 'wp_operations_init', [ Acme_XmlRpc::class, 'bind' ] );
```

`method()` alone keeps the default parameters. `params()` names them: `blog_id`, `username`, `password`, an input
property, or `*`. A malformed name throws `InvalidArgumentException`; a method name outside the rules (or in `system.`)
throws `SpeedySpec\Operation\RegistrationException`. Whether each property is an input is checked when the operations
are exposed.

## The `wp_xmlrpc_server` shim

`wp_xmlrpc_server` extends `SpeedySpec_TrunkXmlRpcServer`, WordPress 7.1.5's class renamed and otherwise unchanged, so
WordPress's method bodies run as they are. It is declared by an autoloader when WordPress first names it, once
`IXR_Server` is loaded. WordPress's own `class-wp-xmlrpc-server.php` must not be loaded: its class would win.

- **The table:** the constructor adds the operations exposed on `xmlrpc` to `xmlrpc_methods`, at the lowest priority,
  as `SpeedySpec\XmlRpc\OperationMethod` entries (callable: calling one runs the operation), keeping the methods
  already there. Plugins then filter the table as today: add a method, replace one, or `unset()` one to hide it.
- **Legacy methods (LegacyHandlerAdapter):** every entry that is not an operation, WordPress's `this:` methods included,
  is an operation named `legacy-xmlrpc/{slug}-{hash}`, restricted to `xmlrpc`, so it never appears on REST. It runs as
  `IXR_Server::call()` runs it: one parameter is passed unwrapped, `this:` methods run on the request's server, and a
  missing method answers IXR's `-32601`. An `IXR_Error` it returns keeps its fault code. Dates reach it as the
  client sent them (`IXR_Date` with its zone), so WordPress stores the instant the client meant.
- **`login()`** goes through the Rust server, so operation and legacy methods share one failed-login latch.
- **`call()` (LegacyCallerAdapter)** reaches any method, WordPress's, a plugin's or an operation's, and returns PHP
  values: `IXR_Date` for dates, `IXR_Base64` for binary, an `IXR_Error` for a fault.
- **`serve_request()`** answers the request through the Rust server.

A new instance gets a new table, under new legacy operation names, while a server still serving keeps its methods: a
plugin method that builds its own `wp_xmlrpc_server` breaks nothing. The failed-login latch belongs to the request and
is shared by every instance in it. `XmlRpcService` builds the Rust server for the request, through
`wp_xmlrpc_server_class` when no instance exists yet, and is the `XmlRpcServerProvider` for `XmlRpcMiddleware`;
`end_request()` clears the request's methods and latch.

## Hooks

`PhpXmlRpcHooks` fires `pre_option_enable_xmlrpc`, `option_enable_xmlrpc` and `xmlrpc_enabled` in
`set_is_enabled()`'s order, `xmlrpc_element_limit`, `xmlrpc_call` (with the request's server) and
`xmlrpc_login_error`. A hook that throws fails closed: XML-RPC counts as disabled, or the call fails with a generic
error; the exception goes to the log, not the client.

## Values

`convert` types PHP values as IXR does: `IXR_Date` is a date, `IXR_Base64` binary, an object a struct of its
properties, an array a list when keyed `0..n` in order and a struct otherwise. Values cross as tagged arrays, so dates,
binary and struct keys such as `"0"` survive.

## Conformance

WordPress's database-free XML-RPC tests (`client.php`, `message.php`, `demo/addTwoNumbers.php`, and `basic.php`
without its database tests), copied from wordpress-develop's 7.1 branch into `tests/php/wordpress/xmlrpc`, run against
the shim by `tests/wordpress.rs`: 36 tests, none expected to fail. A test that fails on purpose goes in
`EXPECTED_FAILURES` with its reason; a failure without one is a defect.
