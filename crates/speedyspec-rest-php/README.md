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
