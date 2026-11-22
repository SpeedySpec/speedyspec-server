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

    /** The REST server of this request, when one exists. */
    public static function server(): mixed
    {
        return ( $GLOBALS['wp_rest_server'] ?? null ) instanceof \WP_REST_Server ? $GLOBALS['wp_rest_server'] : null;
    }

    /** The request's legacy routes after rest_endpoints, as trunk's get_routes() gives them; null before a server. */
    public static function legacy_routes(): ?array
    {
        $server = self::server();
        return $server ? $server->speedyspecLegacyRoutes() : null;
    }

    /** On wp_operations_init: builds WordPress's REST server, which fires rest_api_init, so legacy routes register. */
    public static function rest_init(): void
    {
        if ( function_exists( 'rest_get_server' ) ) {
            rest_get_server();
        }
    }

    /** A WP_REST_Request's parts, as plain arrays, read through its public getters (so any request works). */
    public static function read_request( \WP_REST_Request $request ): array
    {
        $extract = [ \SpeedySpec\Operation\Internal\Mapper::class, 'extract' ];
        $json    = $request->is_json_content_type();
        return [
            'method'   => (string) $request->get_method(),
            'route'    => (string) $request->get_route(),
            'url'      => $extract( (array) $request->get_url_params() ),
            'query'    => $extract( (array) $request->get_query_params() ),
            'body'     => $extract( (array) $request->get_body_params() ),
            'json'     => $json ? $extract( (array) ( $request->get_json_params() ?? [] ) ) : [],
            'json_body' => $json,
            'defaults' => $extract( (array) $request->get_default_params() ),
            'headers'  => (array) $request->get_headers(),
            'raw'      => (string) $request->get_body(),
        ];
    }

    /** A WP_REST_Response from a Rust response's parts. */
    public static function response_from( array $parts ): \WP_REST_Response
    {
        $response = new \WP_REST_Response( $parts['data'], $parts['status'], $parts['headers'] );
        foreach ( $parts['links'] as $link ) {
            $response->add_link( $link['rel'], $link['href'], $link['embeddable'] ? [ 'embeddable' => true ] : [] );
        }
        if ( null !== $parts['route'] ) {
            $response->set_matched_route( $parts['route'] );
        }
        if ( null !== $parts['handler'] ) {
            $response->set_matched_handler( $parts['handler'] );
        }
        return $response;
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
