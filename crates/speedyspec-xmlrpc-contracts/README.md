# speedyspec-xmlrpc-contracts

The XML-RPC transport's contracts, apart from any implementation, so each part can be replaced.

## Value objects

- `XmlRpcBinding { method, params }`: the method name that serves an operation and its positional parameters. A
  parameter is `blog_id` (read and ignored), `username`, `password`, an input property, or `*`, one struct holding
  every input property no other parameter names. Method names are dotted words (`acme.editReport`); the `system.`
  namespace is reserved. Stored as `{method, params: [names]}`; a stored binding with no `params` takes the default
  mapping's.
- `MethodCall { method, params }` and `MethodResponse::{Success, Fault}`: what the codec reads and writes.
- `Fault { code, message }`: WordPress's `IXR_Error`. `Fault::from_error` keeps an error's own `details.fault_code`
  (an `IXR_Error`'s code, kept through a legacy method) and otherwise asks the fault mapping.
- `MethodEntry { method, operation, legacy }`: one row of the served method table. `legacy` marks an operation an
  adapter registered for an old-style method, which receives its positional parameters as they came.

## Ports

| Port | What it decides | Implementations |
|---|---|---|
| `XmlRpcMapping` (Strategy) | an operation's default method and parameters | `DefaultXmlRpcMapping` |
| `FaultMapping` (Strategy) | the fault code of an error category | `DefaultFaultMapping` |
| `XmlRpcLogin` | the caller for a username and password | `PhpXmlRpcLogin` (`wp_authenticate()`) |
| `XmlRpcHooks` | `xmlrpc_enabled`, `xmlrpc_element_limit`, `xmlrpc_methods`, `xmlrpc_call`, `xmlrpc_login_error` | `NoXmlRpcHooks` (WordPress's defaults), `PhpXmlRpcHooks` |
