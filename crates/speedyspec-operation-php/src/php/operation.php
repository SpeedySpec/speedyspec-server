declare(strict_types=1);

namespace SpeedySpec\Operation {

    /**
     * Error categories: gRPC's canonical codes, which every transport maps to its own (HTTP status, fault code, …).
     */
    enum Category: string
    {
        case NotFound          = 'not_found';
        case PermissionDenied  = 'permission_denied';
        case Unauthenticated   = 'unauthenticated';
        case InvalidArgument   = 'invalid_argument';
        case AlreadyExists     = 'already_exists';
        case Conflict          = 'conflict';
        case ResourceExhausted = 'resource_exhausted';
        case Unavailable       = 'unavailable';
        case Internal          = 'internal';
    }

    /**
     * A type or operation could not be registered: the message lists every reason.
     */
    class RegistrationException extends \RuntimeException
    {
    }

    /**
     * Starts operations.
     */
    final class Operation
    {
        public static function query( string $name ): OperationBuilder
        {
            return new OperationBuilder( $name, 'query' );
        }

        public static function command( string $name ): OperationBuilder
        {
            return new OperationBuilder( $name, 'command' );
        }

        public static function subscription( string $name ): OperationBuilder
        {
            return new OperationBuilder( $name, 'subscription' );
        }
    }

    final class OperationBuilder
    {
        private ?array $resource = null;
        private bool $idempotent = false;
        private bool $destructive = false;
        private ?string $paginated = null;
        private ?array $input = null;
        private ?array $output = null;
        private array $errors = [];
        private mixed $permission = null;
        private bool $public = false;
        private ?array $label = null;
        private ?string $deprecated = null;
        private ?array $transports = null;

        public function __construct( private readonly string $name, private readonly string $kind )
        {
        }

        /** The resource type and action this operation works on, and its identity field(s). */
        public function resource( string $type, string $action, string|array $identity = [] ): self
        {
            $this->resource = [ $type, $action, array_values( (array) $identity ) ];
            return $this;
        }

        public function idempotent(): self
        {
            $this->idempotent = true;
            return $this;
        }

        public function destructive(): self
        {
            $this->destructive = true;
            return $this;
        }

        /** `cursor` or `offset`. */
        public function paginated( string $style ): self
        {
            $this->paginated = $style;
            return $this;
        }

        /** The input value object class (`null`: a plain array) and its schema. */
        public function input( ?string $class, Schema $schema ): self
        {
            $this->input = [ $class, $schema ];
            return $this;
        }

        /** The output value object class (`null`: a plain array) and its schema. */
        public function output( ?string $class, Schema $schema ): self
        {
            $this->output = [ $class, $schema ];
            return $this;
        }

        public function error( string $code, Category $category ): self
        {
            $this->errors[] = [ $code, $category->value ];
            return $this;
        }

        /** Who may call it: `$check( Caller $caller, $input ): bool`. */
        public function permission( callable $check ): self
        {
            $this->permission = $check;
            return $this;
        }

        /** Anyone may call it. */
        public function public(): self
        {
            $this->public = true;
            return $this;
        }

        public function label( string $title, string $description ): self
        {
            $this->label = [ $title, $description ];
            return $this;
        }

        public function deprecated( string $reason ): self
        {
            $this->deprecated = $reason;
            return $this;
        }

        /** Only these transports expose it; without this call, every transport does. */
        public function transports( string ...$names ): self
        {
            $this->transports = $names;
            return $this;
        }

        public function register(): void
        {
            Native\register_operation(
                [
                    'name'        => $this->name,
                    'kind'        => $this->kind,
                    'resource'    => $this->resource,
                    'idempotent'  => $this->idempotent,
                    'destructive' => $this->destructive,
                    'paginated'   => $this->paginated,
                    'input'       => $this->contract( $this->input, 'input' ),
                    'output'      => $this->contract( $this->output, 'output' ),
                    'errors'      => $this->errors,
                    'permission'  => $this->permission,
                    'public'      => $this->public,
                    'label'       => $this->label,
                    'deprecated'  => $this->deprecated,
                    'transports'  => $this->transports,
                ]
            );
        }

        private function contract( ?array $contract, string $side ): ?array
        {
            if ( null === $contract ) {
                return null;
            }
            [ $class, $schema ] = $contract;
            return [ 'class' => $class, 'schema' => $schema->toArray(), 'shape' => Internal\Shape::of( $class, $side ) ];
        }
    }

    /**
     * The registered operations.
     */
    final class Operations
    {
        /** Every type and operation as a specification document (JSON Schema plus operation traits). */
        public static function document(): array
        {
            return Native\document();
        }
    }
}

namespace {

    /**
     * Registers an operation built with `SpeedySpec\Operation\Operation`.
     */
    function wp_register_operation( \SpeedySpec\Operation\OperationBuilder $operation ): void
    {
        $operation->register();
    }

    /**
     * Limits the operation `$name` to the named transports.
     *
     * @param list<string> $transports
     */
    function wp_operation_transports( string $name, array $transports ): void
    {
        \SpeedySpec\Operation\Native\restrict( $name, array_values( $transports ) );
    }
}
