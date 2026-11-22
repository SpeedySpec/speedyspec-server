# speedyspec-operation-php

The unified operation API from PHP. `OperationArea` is a native area of the PHP runtime: at every request start it
declares the facade, and its native functions register into, and dispatch through, the Rust registries and dispatcher
of `speedyspec-operation` and `speedyspec-operation-dispatch`.

## What PHP gets

| Name | Role |
|---|---|
| `S`, `Schema` | Schemas, compiled to the JSON Schema 2020-12 the registries read |
| `Type::resource()`, `Type::ref()` | Named resource types with relations, and references to them |
| `Operation::query()`, `::command()`, `::subscription()` | Operation builders; `->register()` |
| `Handler::for( $name, $callable )` | Binds a handler for this request, for any operation: one with Rust contracts gets its input as a read-only `Proxy` and may return an array or value object, validated like any output |
| `Caller` | Who is calling; `->can( $capability, ...$args )` |
| `Operations::call( $name, $input, $user )` | Calls an operation from inside the application (transport `internal`) and returns its output as an array |
| `Operations::document()` | The specification document |
| `OperationException`, `RegistrationException`, `Category` | Errors |
| `wp_register_operation()`, `wp_operation_transports()` | The `wp_` facade functions |

## Registration lifecycle

Registration belongs on the `wp_operations_init` action. The area fires it once per request, the first time the
request needs operations (a call, a document, a transport), never on requests that do not.

Nothing initializes before WordPress has loaded its plugins: until `do_action` exists and `init` has fired, the
operations answer `registry_not_ready` (`unavailable`), and nothing is closed. So a call made too early cannot close
the registries before later plugins hook `wp_operations_init`.

- The first time in a worker, for a site, it fills that site's registries, then closes them. Closing drops only the
  definitions that a cross-reference error names, and keeps the errors in `Site::problems()`. So one plugin's broken
  relation costs only its own operations.
- On later requests it runs again. Re-registering an identical definition is a no-op, and so is re-registering
  one that closing dropped. A different one throws `RegistrationException`. What the re-run does is bind that
  request's PHP callables. If a re-run throws, the site keeps serving with the callables bound so far, and the
  failure is logged.
- `wp_operation_transports()` replaces an operation's own transport list, survives its re-registration, and works
  whether it runs before or after the operation registers. Closing reports a restriction whose operation never
  registered.
- A site is a blog plus a fingerprint of its active plugins and theme (`PhpSiteKey`). A new fingerprint replaces the
  blog's site, which rebuilds its registries. A site whose registration never finished, for example because
  `wp_operations_init` threw, is discarded at request end, and the next request rebuilds it.

**Why callables are per request:** PHP forgets every value at the end of a request, so a closure or object Rust held
would dangle. Specifications (schemas, class names, traits) live for the worker. Permission callbacks and handlers live
for the request that bound them. A request that skips an operation's registration gets
`permission_callback_missing` for it. A PHP handler bound with `Handler::for` replaces a Rust handler only until the
request ends.

## Value objects

Input classes are built by constructor parameter name, and output classes are read by their public properties
(`Internal\Mapper`).

- `DateTimeInterface` parameters receive `DateTimeImmutable` in UTC. Dates cross as `Y-m-d\TH:i:s`.
- Nested value object parameters are built recursively.
- Backed enums cross as their values.
- `null` as the class means plain arrays.

At registration, reflection describes each class, and Rust compares that with the schema: property names, types,
required versus optional, nullable. Every disagreement is listed in the `RegistrationException`.

## Permission callbacks

Only `true` allows. `false`, a `WP_Error` or any other value denies (`operation_unauthenticated` for anonymous
callers, `operation_forbidden` otherwise). `Caller::can()` takes plain arguments: an object (a `WP_Post`, say) cannot
cross the `Identity` port, so it denies and logs. Pass IDs.

## Errors

- A handler or permission callback that throws `OperationException` fails with that exception's code, category,
  message and details.
- Any other `Throwable` is written to `error_log()` and becomes `handler_failed`/`internal` with the message
  `The operation failed.`, so no path or query reaches a client.
- `Operations::call` throws `OperationException` for every failure, including `registry_not_ready` and
  `operations_init_failed`. An init failure's details go to the log, never into the exception.
- `null` for an optional, non-nullable property means "absent" and is removed before validation, at any depth. A
  nullable property keeps its `null`.

## Limits

- Document export lists properties alphabetically (`serde_json` objects), though the registries keep declaration order.
- `Caller::can()` in PHP checks through the area's `Identity`.
