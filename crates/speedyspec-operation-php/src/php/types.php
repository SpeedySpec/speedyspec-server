declare(strict_types=1);

namespace SpeedySpec\Operation;

/**
 * Named resource types, declared once and referenced by name.
 */
final class Type
{
    public static function resource( string $name ): TypeBuilder
    {
        return new TypeBuilder( $name );
    }

    /** A reference to the registered type `$name`. */
    public static function ref( string $name ): Schema
    {
        return Schema::of( [ '$ref' => '#/types/' . $name ] );
    }
}

final class TypeBuilder
{
    /** @var array<string, Schema> */
    private array $fields = [];
    private array $relations = [];

    public function __construct( private readonly string $name )
    {
    }

    public function field( string $name, Schema $schema ): self
    {
        $this->fields[ $name ] = $schema;
        return $this;
    }

    /**
     * `$field` is resolved by `$operation`, whose input fields (keys of `$args`) are filled from this type's fields
     * (values of `$args`).
     */
    public function relation( string $field, string $operation, array $args ): self
    {
        $this->relations[] = [ $field, $operation, $args ];
        return $this;
    }

    public function register(): void
    {
        $fields = [];
        foreach ( $this->fields as $name => $schema ) {
            $fields[ $name ] = [ 'schema' => $schema->toArray(), 'required' => $schema->isRequired() ];
        }
        Native\register_type( [ 'name' => $this->name, 'fields' => $fields, 'relations' => $this->relations ] );
    }
}
