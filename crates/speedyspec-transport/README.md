# speedyspec-transport

The unified operation API's transport layer: how a protocol (REST, XML-RPC, Abilities now; WebSocket and GraphQL
later) exposes the operations declared in `speedyspec-operation` without the specification, the core or plugins
knowing about it.

## Crates

| Crate | Role |
|---|---|
| `speedyspec-transport-contracts` | The ports: `Transport` (with its default mapping), `BindingStore`, `ExposureLookup`, `Authenticator`, `Codec`, `TransportHooks`, `StageHooks`; plus `TransportName`, `DecodedCall`, `ExposedOperation` and `ExposureError`. Depends only on `speedyspec-operation-contracts` |
| `speedyspec-transport` | `MemoryBindingStore`, `TransportRegistry` and the `Exposure` it computes, `StageMiddleware`, and the `Pipeline` every transport serves through |
| `speedyspec-transport-hooks-wp` | `WpTransportHooks`, the `TransportHooks` port over `speedyspec_wp_hooks::HookApi`, with `to_hook`/`from_hook` conversions |

## Exposure

- An operation with no transport restriction is exposed on every installed transport, including one installed later.
- An explicit binding wins over the transport's default mapping, and is checked by the transport.
- A transport with no default mapping for an operation, and no explicit binding, does not expose it.
- A binding for an unknown operation, for an installed transport the operation is restricted away from, or that its
  transport refuses, is skipped and reported in `Exposure::problems`; everything else is still served. Only an open
  registry makes `expose()` fail. A binding for a transport that is not installed is kept and ignored.
- A filter through `WpTransportHooks` that changes nothing returns the neutral value it was given, so dates, binary
  and empty maps keep their shape.

## Serving a request

`Pipeline::serve` runs, in order: authenticate (the pipeline then sets the caller's transport itself), decode, check
the transport exposes the operation (`not_found` otherwise), dispatch, encode. Any failure is encoded in the
transport's error form.

Inside dispatch, hooks follow WordPress's order: authentication hooks (in the transport, before dispatch), the
permission check, the transport's before-handler hooks (`StageMiddleware`, which may answer the call itself), the
handler, then the after-handler hooks (in the transport, on the neutral output).

## Patterns

Ports and Adapters, Strategy (default mapping), Registry (transports), Template Method (the pipeline), Chain of
Responsibility (stage middleware), Adapter (WordPress hooks).

## Binary values

A codec must hand the dispatcher binary values as `Value::Binary`, decoding its own text form first (base64 in JSON,
XML-RPC's `base64`). The dispatcher's `normalize` turns any remaining string under a `binary` schema into that
string's bytes, which is right for PHP strings and wrong for undecoded base64.
