declare(strict_types=1);

namespace SpeedySpec\Operation;

/**
 * One schema, built by `S` and refined by its constraint methods. `toArray()` gives the JSON Schema 2020-12 the
 * Rust registries read.
 */
final class Schema
{
    private bool $required = false;
    private bool $nullable = false;

    private function __construct( private array $json )
    {
    }

    /** @internal Use `S`. */
    public static function of( array $json ): self
    {
        return new self( $json );
    }

    /** The property must be present (inside `S::object()` or a type's fields). */
    public function required(): self
    {
        $this->required = true;
        return $this;
    }

    public function isRequired(): bool
    {
        return $this->required;
    }

    /** `null` is allowed as well. */
    public function nullable(): self
    {
        $this->nullable = true;
        return $this;
    }

    public function description( string $description ): self
    {
        return $this->with( 'description', $description );
    }

    public function enum( array $values ): self
    {
        return $this->with( 'enum', array_values( $values ) );
    }

    public function minimum( int|float $value ): self
    {
        return $this->with( 'minimum', $value );
    }

    public function maximum( int|float $value ): self
    {
        return $this->with( 'maximum', $value );
    }

    public function minLength( int $value ): self
    {
        return $this->with( 'minLength', $value );
    }

    public function maxLength( int $value ): self
    {
        return $this->with( 'maxLength', $value );
    }

    public function pattern( string $pattern ): self
    {
        return $this->with( 'pattern', $pattern );
    }

    public function format( string $format ): self
    {
        return $this->with( 'format', $format );
    }

    public function minItems( int $value ): self
    {
        return $this->with( 'minItems', $value );
    }

    public function maxItems( int $value ): self
    {
        return $this->with( 'maxItems', $value );
    }

    public function uniqueItems(): self
    {
        return $this->with( 'uniqueItems', true );
    }

    /** No properties beyond the declared ones. */
    public function closed(): self
    {
        return $this->with( 'additionalProperties', false );
    }

    /** The property whose value names a `oneOf` branch. */
    public function discriminator( string $property ): self
    {
        return $this->with( 'discriminator', [ 'propertyName' => $property ] );
    }

    public function toArray(): array
    {
        $json = $this->json;
        if ( ! $this->nullable ) {
            return $json;
        }
        if ( isset( $json['type'] ) && is_string( $json['type'] ) ) {
            $json['type'] = [ $json['type'], 'null' ];
            return $json;
        }
        if ( isset( $json['$ref'] ) ) {
            return [ 'oneOf' => [ [ '$ref' => $json['$ref'] ], [ 'type' => 'null' ] ] ];
        }
        if ( isset( $json['oneOf'] ) ) {
            $json['oneOf'][] = [ 'type' => 'null' ];
        }
        return $json;
    }

    private function with( string $keyword, mixed $value ): self
    {
        $this->json[ $keyword ] = $value;
        return $this;
    }
}

/**
 * Starts schemas.
 */
final class S
{
    public static function any(): Schema
    {
        return Schema::of( [] );
    }

    public static function boolean(): Schema
    {
        return Schema::of( [ 'type' => 'boolean' ] );
    }

    public static function integer(): Schema
    {
        return Schema::of( [ 'type' => 'integer' ] );
    }

    public static function number(): Schema
    {
        return Schema::of( [ 'type' => 'number' ] );
    }

    public static function string(): Schema
    {
        return Schema::of( [ 'type' => 'string' ] );
    }

    public static function dateTime(): Schema
    {
        return Schema::of( [ 'type' => 'string', 'format' => 'date-time' ] );
    }

    /** Bytes, of one media type or (`null`) any. */
    public static function binary( ?string $mediaType = null ): Schema
    {
        $json = [ 'type' => 'string', 'contentEncoding' => 'base64' ];
        if ( null !== $mediaType ) {
            $json['contentMediaType'] = $mediaType;
        }
        return Schema::of( $json );
    }

    public static function array( Schema $items ): Schema
    {
        return Schema::of( [ 'type' => 'array', 'items' => $items->toArray() ] );
    }

    /**
     * An object with these properties, in order.
     *
     * @param array<string, Schema> $properties Mark properties `required()` on their own schemas.
     */
    public static function object( array $properties ): Schema
    {
        $json     = [ 'type' => 'object', 'properties' => [] ];
        $required = [];
        foreach ( $properties as $name => $schema ) {
            $json['properties'][ $name ] = $schema->toArray();
            if ( $schema->isRequired() ) {
                $required[] = $name;
            }
        }
        if ( $required ) {
            $json['required'] = $required;
        }
        return Schema::of( $json );
    }

    /** @param list<Schema> $variants */
    public static function oneOf( array $variants ): Schema
    {
        return Schema::of( [ 'oneOf' => array_map( static fn ( Schema $s ): array => $s->toArray(), array_values( $variants ) ) ] );
    }

    /** JSON Schema the helpers do not cover, limited to the keywords the registries read. */
    public static function raw( array $json ): Schema
    {
        return Schema::of( $json );
    }
}
