declare(strict_types=1);

namespace SpeedySpec\Operation\Internal;

use SpeedySpec\Operation\RegistrationException;

/**
 * A value object class as reflection sees it, for the registration check against its schema.
 *
 * @internal
 */
final class Shape
{
    /** The constructor parameters (`input`) or public properties (`output`) of `$class`. */
    public static function of( ?string $class, string $side ): ?array
    {
        if ( null === $class ) {
            return null;
        }
        if ( ! class_exists( $class ) ) {
            throw new RegistrationException( "the value object class {$class} does not exist" );
        }
        $reflection = new \ReflectionClass( $class );
        $members    = [];
        if ( 'input' === $side ) {
            foreach ( $reflection->getConstructor()?->getParameters() ?? [] as $parameter ) {
                $members[] = self::member( $parameter->getName(), $parameter->getType(), $parameter->isOptional() );
            }
        } else {
            foreach ( $reflection->getProperties( \ReflectionProperty::IS_PUBLIC ) as $property ) {
                if ( ! $property->isStatic() ) {
                    $members[] = self::member( $property->getName(), $property->getType(), $property->hasDefaultValue() );
                }
            }
        }
        return [ 'class' => $reflection->getName(), 'members' => $members ];
    }

    private static function member( string $name, ?\ReflectionType $type, bool $default ): array
    {
        return [
            'name'     => $name,
            'types'    => self::names( $type ),
            'nullable' => null === $type || $type->allowsNull(),
            'default'  => $default,
        ];
    }

    private static function names( ?\ReflectionType $type ): array
    {
        if ( null === $type ) {
            return [];
        }
        if ( $type instanceof \ReflectionNamedType ) {
            $name  = strtolower( $type->getName() );
            $names = [ $name ];
            if ( $type->allowsNull() && 'null' !== $name && 'mixed' !== $name ) {
                $names[] = 'null';
            }
            return $names;
        }
        if ( $type instanceof \ReflectionUnionType ) {
            $names = [];
            foreach ( $type->getTypes() as $member ) {
                array_push( $names, ...self::names( $member ) );
            }
            return array_values( array_unique( $names ) );
        }
        return [ 'object' ];
    }
}

/**
 * Builds value objects from plain arrays and reads them back.
 *
 * @internal
 */
final class Mapper
{
    /** A `$class` instance from `$value`, by constructor parameter name; `$value` itself when there is no class. */
    public static function hydrate( ?string $class, mixed $value ): mixed
    {
        if ( null === $class || ! is_array( $value ) ) {
            return $value;
        }
        $constructor = ( new \ReflectionClass( $class ) )->getConstructor();
        if ( null === $constructor ) {
            return new $class();
        }
        $args = [];
        foreach ( $constructor->getParameters() as $parameter ) {
            $name = $parameter->getName();
            if ( array_key_exists( $name, $value ) ) {
                $args[ $name ] = self::cast( $parameter->getType(), $value[ $name ] );
            } elseif ( ! $parameter->isOptional() ) {
                $args[ $name ] = null;
            }
        }
        return new $class( ...$args );
    }

    /** `$value` as plain arrays and scalars: value objects by their public properties, dates as UTC ISO 8601. */
    public static function extract( mixed $value ): mixed
    {
        if ( is_array( $value ) ) {
            return array_map( [ self::class, 'extract' ], $value );
        }
        if ( $value instanceof \DateTimeInterface ) {
            return \DateTimeImmutable::createFromInterface( $value )
                ->setTimezone( new \DateTimeZone( 'UTC' ) )
                ->format( 'Y-m-d\TH:i:s' );
        }
        if ( $value instanceof \BackedEnum ) {
            return $value->value;
        }
        if ( is_object( $value ) ) {
            return array_map( [ self::class, 'extract' ], get_object_vars( $value ) );
        }
        return $value;
    }

    private static function cast( ?\ReflectionType $type, mixed $value ): mixed
    {
        if ( null === $value || ! $type instanceof \ReflectionNamedType || $type->isBuiltin() ) {
            return $value;
        }
        $class = $type->getName();
        if ( is_string( $value ) && is_a( $class, \DateTimeInterface::class, true ) ) {
            $utc = new \DateTimeZone( 'UTC' );
            return \DateTime::class === $class ? new \DateTime( $value, $utc ) : new \DateTimeImmutable( $value, $utc );
        }
        if ( is_subclass_of( $class, \BackedEnum::class ) ) {
            return $class::from( $value );
        }
        if ( is_array( $value ) && class_exists( $class ) ) {
            return self::hydrate( $class, $value );
        }
        return $value;
    }
}

/**
 * Which site this request serves, and its active plugins and theme: a change of either rebuilds the site's registries.
 *
 * @internal
 */
final class SiteKey
{
    public static function current(): array
    {
        $blog  = function_exists( 'get_current_blog_id' ) ? (int) get_current_blog_id() : 1;
        $parts = [];
        if ( function_exists( 'get_option' ) ) {
            $parts[] = (string) json_encode( get_option( 'active_plugins', [] ) );
            $parts[] = (string) get_option( 'stylesheet', '' );
            $parts[] = (string) get_option( 'template', '' );
        }
        if ( function_exists( 'is_multisite' ) && is_multisite() && function_exists( 'get_site_option' ) ) {
            $parts[] = (string) json_encode( array_keys( (array) get_site_option( 'active_sitewide_plugins', [] ) ) );
        }
        return [ $blog, implode( "\n", $parts ) ];
    }
}
