declare(strict_types=1);

namespace SpeedySpec\Rest;

/**
 * Binds operations to REST routes.
 */
final class Rest
{
    /** Starts the REST binding of `$operation`: `->route( $namespace, $route )->methods( 'GET', … )`. */
    public static function bind( string $operation ): RestBinder
    {
        return new RestBinder( $operation );
    }
}

final class RestBinder
{
    private ?string $namespace = null;
    private ?string $route = null;

    public function __construct( private readonly string $operation )
    {
    }

    /** The namespace (`vendor/v1`) and route pattern (`/reports/(?P<id>\d+)`). */
    public function route( string $namespace, string $route ): self
    {
        $this->namespace = $namespace;
        $this->route     = $route;
        return $this;
    }

    /** The methods served; stores the binding. */
    public function methods( string ...$methods ): void
    {
        Native\bind( $this->operation, (string) $this->namespace, (string) $this->route, $methods );
    }
}

namespace SpeedySpec\Rest\Internal;

/**
 * Between WordPress's objects and the Rust REST request and response.
 *
 * @internal
 */
final class Adapter
{
    /** What a value a hook returned is, as plain arrays. */
    public static function describe( mixed $value ): array
    {
        if ( null === $value ) {
            return [ 'kind' => 'none' ];
        }
        $extract = [ \SpeedySpec\Operation\Internal\Mapper::class, 'extract' ];
        if ( class_exists( 'WP_Error', false ) && $value instanceof \WP_Error ) {
            return [
                'kind'    => 'error',
                'code'    => (string) $value->get_error_code(),
                'message' => (string) $value->get_error_message(),
                'data'    => $extract( $value->get_error_data() ),
            ];
        }
        if ( class_exists( 'WP_HTTP_Response', false ) && $value instanceof \WP_HTTP_Response ) {
            $headers = [];
            foreach ( (array) $value->get_headers() as $name => $header ) {
                $headers[ (string) $name ] = is_array( $header ) ? implode( ', ', $header ) : (string) $header;
            }
            $links = [];
            if ( method_exists( $value, 'get_links' ) ) {
                foreach ( (array) $value->get_links() as $rel => $entries ) {
                    foreach ( (array) $entries as $entry ) {
                        $links[] = [
                            'rel'        => (string) $rel,
                            'href'       => (string) ( $entry['href'] ?? '' ),
                            'embeddable' => ! empty( $entry['attributes']['embeddable'] ),
                        ];
                    }
                }
            }
            return [
                'kind'    => 'response',
                'handle'  => null,
                'data'    => $extract( $value->get_data() ),
                'status'  => (int) $value->get_status(),
                'headers' => $headers,
                'links'   => $links,
            ];
        }
        return [ 'kind' => 'value', 'data' => $extract( $value ) ];
    }

    /** On wp_operations_init: builds WordPress's REST server, which fires rest_api_init, so legacy routes register. */
    public static function rest_init(): void
    {
        if ( function_exists( 'rest_get_server' ) ) {
            rest_get_server();
        }
    }

    public static function error( string $code, string $message, array $data ): mixed
    {
        return class_exists( 'WP_Error', false )
            ? new \WP_Error( $code, $message, $data )
            : [ 'code' => $code, 'message' => $message, 'data' => $data ];
    }

    public static function request( int $handle ): mixed
    {
        return class_exists( \SpeedySpec\Rest\RequestProxy::class, false ) ? new \SpeedySpec\Rest\RequestProxy( $handle ) : null;
    }

    public static function response( int $handle ): mixed
    {
        return class_exists( \SpeedySpec\Rest\ResponseProxy::class, false )
            ? new \SpeedySpec\Rest\ResponseProxy( $handle )
            : \SpeedySpec\Rest\Native\response_get( $handle, 'data' );
    }
}
