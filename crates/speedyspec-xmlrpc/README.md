# speedyspec-xmlrpc

The XML-RPC transport: `xmlrpc.php` served by Rust, for every operation and, through `speedyspec-xmlrpc-php`, for
WordPress's and plugins' own methods.

Patterns: Adapter (the codec), Strategy (`DefaultXmlRpcMapping`, `DefaultFaultMapping`), Template Method
(`XmlRpcServer::serve`), Registry (the per-request method table).

## The codec

`codec::parse_call` and `codec::write_response` read and write XML-RPC as `IXR_Server` speaks it.

| XML-RPC | Neutral value |
|---|---|
| `int`, `i4`, `i8` | `Int` |
| `double` | `Float` |
| `boolean` (`0`/`1`, also `true`/`false`) | `Bool` |
| `string`, or bare text in `<value>` | `String` |
| `dateTime.iso8601` | `DateTime`, normalized to UTC |
| `base64` (whitespace ignored) | `Binary` |
| `struct` | `Map`, in order |
| `array` | `List` |
| `nil` | `Null` |

Dates are read as `YYYYMMDDTHH:MM:SS`, `YYYY-MM-DDTHH:MM:SS` or `YYYYMMDDTHHMMSS`, with an optional `Z` or offset, and
written as `YYYYMMDDTHH:MM:SS`. `Null` is written as an empty `<string>` and every integer as `<int>`, as IXR writes
them. Scalars are read strictly: a non-numeric `<int>` is a parse fault, where IXR would cast it to 0.

The request body is the client's, so reading is defensive:

- a `<!DOCTYPE` is refused, so no entity is ever expanded and nothing external is read;
- more than twice `xmlrpc_element_limit` elements (30000 by default) is refused, as IXR refuses it;
- nesting deeper than 256 values is refused, so a deep document cannot exhaust the stack;
- a body that is not UTF-8 is refused.

Each answers IXR's fault `-32700 parse error. not well formed`; a document that is not a `methodCall` answers `-32600`.

## Default method names

`DefaultXmlRpcMapping` names methods after WordPress's own, under the operation's vendor (characters other than letters,
digits and `_` become `_`):

| Operation | Method | Params |
|---|---|---|
| list | `{vendor}.get{Plural}` | `blog_id, username, password, *` |
| get | `{vendor}.get{Type}` | `blog_id, username, password, {identity…}, *` |
| create | `{vendor}.new{Type}` | `blog_id, username, password, *` |
| update | `{vendor}.edit{Type}` | `blog_id, username, password, {identity…}, *` |
| delete | `{vendor}.delete{Type}` | `blog_id, username, password, {identity…}` |
| custom action | `{vendor}.{action}{Type}` | `blog_id, username, password, {identity…}, *` |
| no resource | `{vendor}.{name after the vendor}` | `blog_id, username, password, *` |

`*` is left out when no input property is left. Subscriptions have no method. Two operations claiming one name: the
first by operation name is served, and the other is logged and skipped.

## Fault codes

| Category | Code |
|---|---|
| InvalidArgument | 400 |
| PermissionDenied | 401 |
| Unauthenticated | 403 |
| NotFound | 404 |
| AlreadyExists, Conflict | 409 |
| ResourceExhausted | 429 |
| Unavailable | 503 |
| Internal | 500 |

An error carrying `details.fault_code` keeps that code. Faults are sent with HTTP 200, as IXR sends them.

## The server

`XmlRpcServer` is one request's server. `serve()` refuses anything but `POST` (405, `Allow: POST`), parses, calls and
writes `text/xml; charset=UTF-8`. `call()`:

- **`system.*`:** `system.listMethods`, `system.getCapabilities` and `system.multicall` answer as IXR does. As in IXR,
  the method table can neither remove nor replace them. A nested `system.multicall` fails with `-32600`.
- **The table:** every operation exposed on `xmlrpc`, then `XmlRpcHooks::methods` (WordPress's `xmlrpc_methods`), built
  once. A method the hooks remove answers `-32601`, directly or inside a multicall, and is not listed.
- **An operation:** too few parameters is 400 `Insufficient arguments passed to this XML-RPC method.`; extra ones are
  ignored; `*` is optional and must be a struct; positional fields win over the same keys in it. With credentials in
  its parameters, the call logs in; without, it is anonymous. Then `xmlrpc_call`, then dispatch.
- **A legacy entry:** dispatched with `{params: [...]}` as it came; its own body logs in and fires `xmlrpc_call`, as
  WordPress's methods do.
- **Login:** 405 `XML-RPC services are disabled on this site.` when disabled; 403 `Incorrect username or password.`
  through `xmlrpc_login_error` on bad credentials; and after one failure, 403 for every later login in the request
  without trying again, as WordPress's `auth_failed` does.

`XmlRpcMiddleware` serves `/xmlrpc.php` (also under a subdirectory) and passes every other path on.
