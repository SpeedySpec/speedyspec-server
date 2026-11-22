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
| `Handler::for( $name, $callable )` | Binds a handler for this request |
| `Caller` | Who is calling; `->can( $capability, ...$args )` |
| `Operations::call( $name, $input, $user )` | Calls an operation from inside the application (transport `internal`) and returns its output as an array |
| `Operations::document()` | The specification document |
| `OperationException`, `RegistrationException`, `Category` | Errors |
| `wp_register_operation()`, `wp_operation_transports()` | The `wp_` facade functions |

## Registration lifecycle

Registration belongs on the `wp_operations_init` action. The area fires it once per request, the first time the
request needs operations (a call, a document, a transport), never on requests that do not.

- The first time in a worker, for a site, it fills that site's registries, then closes them. Closing drops only the
  definitions that a cross-reference error names, and keeps the errors in `Site::problems()`. So one plugin's broken
  relation costs only its own operations.
- On later requests it runs again. Re-registering an identical definition is a no-op, and a different one throws
  `RegistrationException`. What the re-run does is bind that request's PHP callables.
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

## Errors

- A handler or permission callback that throws `OperationException` fails with that exception's code, category,
  message and details.
- Any other `Throwable` is written to `error_log()` and becomes `handler_failed`/`internal` with the message
  `The operation failed.`, so no path or query reaches a client.
- `Operations::call` throws `OperationException` for every failure.

## Limits

- A PHP handler cannot handle an operation whose contracts are Rust structs: it fails with
  `handler_contract_mismatch`. Lifting that needs dispatch to offer handlers the neutral value, with output still
  validated.
- Document export lists properties alphabetically (`serde_json` objects), though the registries keep declaration order.
- `Caller::can()` in PHP checks through the area's `Identity`.
