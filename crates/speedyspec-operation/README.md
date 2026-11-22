# speedyspec-operation

The unified operation API: each public client operation is declared once, as a protocol-neutral contract, bound to
one handler, and exposed by any number of transports (REST, XML-RPC, Abilities now; WebSocket and GraphQL later). The
model is WSDL or Smithy: the specification says what an operation accepts, returns and means, never how a wire format
carries it.

## Crates

| Crate | Role |
|---|---|
| `speedyspec-operation-contracts` | Plain value objects and ports, depending on no SpeedySpec crate: the neutral `Value`, `Schema`, `TypeSpec` and relations, `OperationSpec`, `OperationError` and its `Category`, `Caller` and the `Identity` port, the `ContractMapper` port, `RawInput`/`RawOutput`, the catalogue ports |
| `speedyspec-operation` | The `S` schema builder, the validator, the `Type` and `Operation` builders, typed mappers, `Registries` with their registration checks, document export, and the contract round-trip test helper |
| `speedyspec-operation-dispatch` | Handlers bound in the service container by operation name, middleware (permission first), and `Dispatcher::call` by name with neutral values |

## A call

1. A transport decodes its wire format to a neutral `Value` and authenticates a `Caller`.
2. The input is validated against the operation's schema, then hydrated into its input value object.
3. Middleware runs around the handler, the permission check first.
4. The handler returns an output value object, which is extracted to a neutral value and validated against the output
   schema; a value that breaks it never leaves.

An operation with no transport restriction is exposed on every transport. Registration closes once everything is
registered; after that, re-registering an identical definition is a no-op and anything else is refused.

## Patterns

Hexagonal (contracts inside, transports as driving adapters), Builder and Value Object (`S`, `Type`, `Operation`),
Registry (`Registries`), Data Mapper (hydrate/extract, JSON export), Command/Query dispatch by key, Chain of
Responsibility (middleware).

`speedyspec-cqrs`'s `Middleware` cannot be driven from another crate (`Next` has no public constructor), so dispatch has
its own `OperationMiddleware`. Handlers still live in the shared container, so rebinding a name replaces a handler.
