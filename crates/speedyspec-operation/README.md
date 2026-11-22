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
| `speedyspec-operation-php` | The PHP facade: `SpeedySpec\Operation\{S, Type, Operation, Handler, Caller, Operations}`, `wp_register_operation`, `wp_operation_transports`, PHP value-object mappers, the `user_can` identity adapter and per-site registries. See its README |

## A call

1. A transport decodes its wire format to a neutral `Value` and authenticates a `Caller`.
2. The input is validated against the operation's schema, then hydrated into its input value object.
3. Middleware runs around the handler, the permission check first.
4. The handler returns an output value object, which is extracted to a neutral value and validated against the output
   schema; a value that breaks it never leaves.

An operation with no transport restriction is exposed on every transport. Registration closes once everything is
registered; after that, re-registering an identical definition is a no-op and anything else is refused. A restriction
applied with `restrict` is remembered, so an identical re-registration keeps it. `close_lenient` closes whatever the
checks find, dropping only the definitions an error names.

Before validation, `normalize` reshapes values whose wire or PHP form is ambiguous: an integral float under `integer`,
a date string under `date-time`, a string under `binary`, and an empty list under an object. It runs on input and on
output.

## From PHP

```php
use SpeedySpec\Operation\{ S, Type, Operation, Handler, Caller, Category };

final class Acme_Reports {
    public static function register(): void {
        Type::resource( 'report' )
            ->field( 'id', S::integer()->required() )
            ->field( 'title', S::string()->required() )
            ->register();

        Operation::query( 'acme/get-report' )
            ->resource( 'report', 'get', 'id' )
            ->input( Acme_Get_Report::class, S::object( [ 'id' => S::integer()->required() ] ) )
            ->output( Acme_Report::class, Type::ref( 'report' ) )
            ->error( 'report_not_found', Category::NotFound )
            ->permission( [ self::class, 'can_read' ] )
            ->register();

        Handler::for( 'acme/get-report', [ self::class, 'get' ] );
    }

    public static function can_read( Caller $caller, Acme_Get_Report $input ): bool {
        return $caller->can( 'read_report', $input->id );
    }

    public static function get( Acme_Get_Report $input, Caller $caller ): Acme_Report {
        return Acme_Report_Store::find( $input->id );
    }
}
add_action( 'wp_operations_init', [ Acme_Reports::class, 'register' ] );
```

## Patterns

Hexagonal (contracts inside, transports as driving adapters), Builder and Value Object (`S`, `Type`, `Operation`),
Registry (`Registries`), Data Mapper (hydrate/extract, JSON export), Command/Query dispatch by key, Chain of
Responsibility (middleware).

`speedyspec-cqrs`'s `Middleware` cannot be driven from another crate (`Next` has no public constructor), so dispatch has
its own `OperationMiddleware`. Handlers still live in the shared container, so rebinding a name replaces a handler.
