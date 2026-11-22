# speedyspec-rest

The unified operation API's REST transport: operations served over `/wp-json` (and `?rest_route=`) from Rust, in
WordPress's shape and hook order. Contracts live in `speedyspec-rest-contracts`; the PHP hooks adapter in
`speedyspec-rest-php`.

## Serve order

`RestServer` runs WordPress trunk's order (`WP_REST_Server::serve_request`, `dispatch`, `respond_to_request`).

| # | Step | Hook (`RestHooks`) |
|---|---|---|
| 1 | Authentication | `authentication_errors` (`rest_authentication_errors`), then the `Authenticator` resolves the caller; an error still passes `rest_post_dispatch` |
| 2 | May answer before matching | `pre_dispatch` (`rest_pre_dispatch`) |
| 3 | Route matching, index routes | `rest_no_route` (404) when nothing matches the path and method |
| 4 | Before the permission check | `before_callbacks` (`rest_request_before_callbacks`); writes to the request reach the handler and are validated again |
| 5 | Permission check | the operation's own |
| 6 | May answer instead of the handler | `dispatch_request` (`rest_dispatch_request`) |
| 7 | The handler | — |
| 8 | Each item of a resource type | `prepare_item` (`rest_prepare_{type}`) |
| 9 | Sees the outcome, data or error | `after_callbacks` (`rest_request_after_callbacks`) |
| 10 | Errors become responses | — |
| 11 | Links and embeds | `_links` from relations; `_embed` (every relation, or `?_embed=author,comments`) through dispatch, each href fetched once per request |
| 12 | The response | `post_dispatch` (`rest_post_dispatch`), then `_fields` |
| 13 | Serving | `pre_serve` (`rest_pre_serve_request`) |

## Default routes

`DefaultRestMapping` (a Strategy) routes an operation by its resource and action, under `{vendor}/v1`. `Rest::bind`
(or any binding in the store) wins over it. A default route that collides with a taken route and method is skipped and
reported in `RestServer::problems()`.

| Operation | Route | Methods |
|---|---|---|
| list | `/{plural}` | GET |
| get | `/{plural}/{identity}` | GET |
| create | `/{plural}` | POST (answers 201) |
| update | `/{plural}/{identity}` | POST, PUT, PATCH |
| delete | `/{plural}/{identity}` | DELETE |
| custom action | `/{plural}/{identity}/{action}` | GET for a query, POST for a command |
| no resource | `/{name after the vendor}` | GET for a query, POST for a command |

Plurals are English (`report` → `reports`, `category` → `categories`). Identity segments are `[\d]+` for integer
fields and `[^/]+` otherwise. `HEAD` is served as `GET` without a body.

## Requests and responses

- **Parameters** follow `WP_REST_Request`: each is read from the first source holding it, in the order JSON body
  (JSON requests), form body (methods with a body), query, URL parameters. So the query beats a URL parameter, and
  body parameters are ignored on `GET`. `set_param()` updates every source holding the key. Queries and form bodies
  are read with PHP's `parse_str` rules (`a[]=1`, `a[b]=1`). Paths are percent-decoded, with `+` kept. WordPress's
  own parameters (`_embed`, `_fields`, `_locale`, `_wpnonce`, `_method`, `_envelope`, `_jsonp`, `_pretty`) never reach
  an operation's input unless it declares them, so closed schemas accept them. Strings are coerced to their schema's kind: `"5"` becomes an integer, `"false"`
  a boolean, `"a,b"` a list.
- **Errors** answer `{code, message, data: {status, …}}`. The status comes from the category (`not_found` 404,
  `permission_denied` 403, `unauthenticated` 401, `invalid_argument` 400, `already_exists`/`conflict` 409,
  `resource_exhausted` 429, `unavailable` 503, `internal` 500), unless `details.status` says otherwise. Invalid input
  is `rest_invalid_param` or `rest_missing_callback_param`; a refused permission is `rest_forbidden` (401 when
  anonymous, 403 otherwise).
- **JSON bodies** keep map order. Binary is base64; dates are `Y-m-d\TH:i:s`.

## Integration points

| Port | Default | Replace with |
|---|---|---|
| `RestHooks` | `NoRestHooks` | `speedyspec_rest_php::PhpRestHooks`, or any implementation |
| `Authenticator<HttpServerRequest>` | — | `speedyspec_rest_php::PhpRestAuthenticator` |
| `RestMapping` | `DefaultRestMapping` | `RestTransport::with_mapping`, or explicit bindings in the `BindingStore` |
| Handlers | the operation's | any handler bound in the container, PHP included |

The route table and the HTTP codec (`decode`, `encode`) are not ports yet: `RestServer` builds its table from the
exposure and uses the free functions. They become ports when a second implementation needs them.

## Not yet

- `RestMiddleware` is not wired into the running server's HTTP pipeline; that belongs to the server crates.
- `OPTIONS` (route schemas), batch requests, JSONP, `_envelope` and CORS headers are not served.
- The legacy shims (`register_rest_route`, `WP_REST_Server`, `rest_do_request`) are Phase 5.
