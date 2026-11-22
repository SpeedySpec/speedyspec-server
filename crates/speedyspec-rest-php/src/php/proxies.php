declare(strict_types=1);

namespace SpeedySpec\Rest;

/**
 * A `WP_REST_Request` whose parameters, headers, method and route are the Rust REST request's, read and written live:
 * a hook's `set_param()` changes what the handler receives.
 */
class RequestProxy extends \WP_REST_Request
{
    public function __construct( private readonly int $speedyspecHandle )
    {
    }

    private function get( string $what, mixed ...$args ): mixed
    {
        return Native\request_get( $this->speedyspecHandle, $what, ...$args );
    }

    private function put( string $what, mixed ...$args ): void
    {
        Native\request_set( $this->speedyspecHandle, $what, ...$args );
    }

    private static function plain( mixed $value ): mixed
    {
        return \SpeedySpec\Operation\Internal\Mapper::extract( $value );
    }

    public function get_method() { return $this->get( 'method' ); }
    public function set_method( $method ) { $this->put( 'method', (string) $method ); }
    public function get_route() { return $this->get( 'route' ); }
    public function set_route( $route ) { $this->put( 'route', (string) $route ); }
    public function get_param( $key ) { return $this->get( 'param', (string) $key ); }
    public function has_param( $key ) { return $this->get( 'has_param', (string) $key ); }
    public function set_param( $key, $value ) { $this->put( 'param', (string) $key, self::plain( $value ) ); }
    public function get_params() { return $this->get( 'params' ); }
    public function get_url_params() { return $this->get( 'url_params' ); }
    public function get_query_params() { return $this->get( 'query_params' ); }
    public function get_body_params() { return $this->get( 'body_params' ); }
    public function get_json_params() { return $this->get( 'json_params' ); }
    public function get_header( $key ) { return $this->get( 'header', (string) $key ); }
    public function get_headers() { return $this->get( 'headers' ); }
    public function set_header( $key, $value ) { $this->put( 'header', (string) $key, $value ); }
    public function get_body() { return $this->get( 'body' ); }
    public function get_attributes() { return []; }

    #[\ReturnTypeWillChange]
    public function offsetExists( $offset ) { return $this->has_param( $offset ); }

    #[\ReturnTypeWillChange]
    public function offsetUnset( $offset ) { $this->put( 'unset', (string) $offset ); }
}

/**
 * A `WP_REST_Response` whose data, status and headers are the Rust REST response's, read and written live.
 */
class ResponseProxy extends \WP_REST_Response
{
    public function __construct( private readonly int $speedyspecHandle )
    {
    }

    /** @internal */
    public function speedyspecHandle(): int
    {
        return $this->speedyspecHandle;
    }

    public function get_data() { return Native\response_get( $this->speedyspecHandle, 'data' ); }
    public function set_data( $data ) { Native\response_set( $this->speedyspecHandle, 'data', \SpeedySpec\Operation\Internal\Mapper::extract( $data ) ); }
    public function get_status() { return Native\response_get( $this->speedyspecHandle, 'status' ); }
    public function set_status( $code ) { Native\response_set( $this->speedyspecHandle, 'status', (int) $code ); }
    public function get_headers() { return Native\response_get( $this->speedyspecHandle, 'headers' ); }
    public function set_headers( $headers ) { Native\response_set( $this->speedyspecHandle, 'headers', (array) $headers ); }
    public function header( $key, $value, $replace = true ) { Native\response_set( $this->speedyspecHandle, 'header', (string) $key, (string) $value, (bool) $replace ); }
    public function get_links() { return Native\response_get( $this->speedyspecHandle, 'links' ); }
}
