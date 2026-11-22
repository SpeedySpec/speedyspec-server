# speedyspec-rest-php

The REST transport's PHP adapter: WordPress's REST hooks, in PHP, over the Rust REST server.

## Proxies

Hooks receive WordPress's own types:

- `SpeedySpec\Rest\RequestProxy extends WP_REST_Request` stands for the Rust `RestRequest` live. Every reader and
  setter of `WP_REST_Request` (parameters by source, `set_param()`, headers, body, method, route, array access) reads
  and writes the Rust request. A `set_param()` in `rest_request_before_callbacks` changes what the handler receives,
  and the new input is validated before the handler runs. File parameters, default parameters and attributes are
  empty, and setting them does nothing.
- `SpeedySpec\Rest\ResponseProxy extends WP_REST_Response` is a real `WP_REST_Response` holding the Rust response's
  data, status, headers and links. Every WordPress method and property works on it (`$response->data['x'] = …`,
  `add_link()`, `header()`), and the server reads it back after each hook.

The proxies are declared in a request once `WP_REST_Request` exists to extend. A request proxy stands for its Rust
request only for its request: used later, it throws `LogicException`. Objects set through a proxy cross as their public
properties.

## A filter that throws

As in WordPress, where an uncaught exception ends the request, a throwing `rest_authentication_errors`,
`rest_pre_dispatch`, `rest_request_before_callbacks`, `rest_dispatch_request`, `rest_request_after_callbacks` or
`rest_post_dispatch` callback fails the request with a generic `rest_hook_failed` error (500). It never lets the
request through. The exception goes to the log, not the client.

## What hooks return

| Returned | Becomes |
|---|---|
| `WP_Error` | an operation error: code and message kept; the category from its `status` (401 `unauthenticated`, 403 `permission_denied`, 404 `not_found`, 409 `conflict`, 429 `resource_exhausted`, 503 `unavailable`, other 4xx `invalid_argument`, else `internal`); its data, `status` included, as details, so the status round-trips |
| `WP_REST_Response` (or the proxy) | the response, with its data, status and headers |
| anything else | data, status 200 |
| `null` | nothing (the hook did not answer) |

An error going into a hook (`rest_request_after_callbacks`) is the `WP_Error` its REST response would carry, such as
`rest_invalid_param` with `params` and `details`.

## Authentication

The server fires `rest_authentication_errors`; `PhpRestAuthenticator` then makes `get_current_user_id()` the caller,
with 0 meaning anonymous.

## Binding routes

```php
use SpeedySpec\Rest\Rest;

final class Acme_Routes {
    public static function bind(): void {
        Rest::bind( 'acme/get-report' )->route( 'acme/v2', '/report/(?P<id>\d+)' )->methods( 'GET' );
    }
}
add_action( 'wp_operations_init', [ Acme_Routes::class, 'bind' ] );
```

A binding is checked when it is made: the namespace is `vendor/version`, the route starts with `/`, the methods are
known, and every route parameter is an input of the operation. A bad one throws
`SpeedySpec\Operation\RegistrationException`.

## Legacy routes (`register_rest_route`)

`WP_REST_Server` is SpeedySpec's shim, declared when WordPress's class is not loaded (never load
`class-wp-rest-server.php` beside it). It extends `SpeedySpec_TrunkRestServer`, a renamed copy of WordPress 7.1.5's
class, so WordPress's own helpers (`response_to_data`, link compaction, envelopes, the index) are unchanged. Route
storage and dispatch go to Rust.

- **What a route becomes:** each endpoint of a `register_rest_route()` route is an operation named
  `legacy-rest/{slug}-{hash}`, restricted to `rest`, with an explicit binding for its exact route.
- **Lifetime:** legacy routes belong to the request's `WP_REST_Server` instance, as in WordPress. A new server starts
  with none.
- **Registration:** `rest_api_init` fires from `wp_operations_init` through WordPress's own `rest_get_server()`.
- **Duplicates:** without `$override` the first handler for a route and method wins, as WordPress's first match does.
  With it, the later one replaces it.
- **Validation in WordPress's words:** before the permission callback, the request gets the arguments' defaults, then
  WordPress's own `has_valid_params()` and `sanitize_params()` logic (required arguments, `validate_callback`,
  `sanitize_callback`, `rest_parse_request_arg` for typed arguments). Errors are WordPress's, such as
  `rest_missing_callback_param` and `rest_invalid_param` with its messages.
- **Permission:** `true` allows; a `WP_Error` is its error; `false` or `null` is `rest_forbidden` (401 or 403); no
  permission callback means public, with WordPress's notice.
- **The callback:** it gets the request proxy. A `WP_REST_Response` keeps its status, headers and links; a `WP_Error`
  keeps its status. A callback that cannot be called is `rest_invalid_handler` (500).

## `dispatch()` and `rest_do_request()`

`WP_REST_Server::dispatch()`, and so WordPress's own `rest_do_request()`, reach every operation, new-style or legacy,
through the Rust REST server.
- **The request:** any `WP_REST_Request` is read through its public getters. The URL parameters, defaults and
  sanitized values are written back to it, as WordPress's dispatch changes the request it is given.
- **The response:** a `WP_REST_Response` with the matched route and handler. Neither function fires
  `rest_post_dispatch`, as in WordPress.
- **The index:** `GET /` is trunk's own `get_index()`. `get_routes()` and `get_namespaces()` list legacy and new-style
  routes together.
- **The hooks:** they receive the request's server and the matched handler.

## `RestService`

`RestService` keeps one REST server per site, serving the site's operations plus the request's legacy routes. It is
rebuilt only when the site's registries, the stored bindings or the legacy routes change, so a site registering the
same routes on every request keeps one server. The server's HTTP wiring can use it as well; that wiring is not done
yet.

## Conformance

`tests/wordpress.rs` runs WordPress's own `rest-server.php` tests (wordpress-develop 7.1, the pinned 7.1.5 release's
code) through the pinned PHPUnit in the embedded PHP, minus the tests that need the database or the factory. Every
failure must be in `EXPECTED_FAILURES` with its reason. At present 119 tests run and 15 fail on purpose: batch
requests, core's own routes, attachments, PHP's `serve_request()` output, an API PHPUnit removed, and the `Allow`
header on `OPTIONS`.
