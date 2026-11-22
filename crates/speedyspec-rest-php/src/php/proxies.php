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
    public function get_header_as_array( $key ) { return $this->get( 'header_list', (string) $key ); }
    public function add_header( $key, $value ) { $this->put( 'add_header', (string) $key, $value ); }
    public function remove_header( $key ) { $this->put( 'remove_header', (string) $key ); }
    public function set_headers( $headers, $override = true ) { $this->put( 'headers', self::plain( (array) $headers ), (bool) $override ); }
    public function set_url_params( $params ) { $this->put( 'url_params', self::plain( (array) $params ) ); }
    public function set_query_params( $params ) { $this->put( 'query_params', self::plain( (array) $params ) ); }
    public function set_body_params( $params ) { $this->put( 'body_params', self::plain( (array) $params ) ); }
    public function set_body( $data ) { $this->put( 'body', (string) $data ); }
    public function get_file_params() { return []; }
    public function set_file_params( $params ) { }
    public function get_default_params() { return $this->get( 'default_params' ); }
    public function set_default_params( $params ) { $this->put( 'default_params', self::plain( (array) $params ) ); }
    public function get_attributes() { return $this->get( 'attributes' ); }

    /**
     * WordPress's own check (WP_REST_Request::has_valid_params), reading through get_param(): required arguments, the
     * arguments' validate_callback, the route's validate_callback.
     */
    public function has_valid_params()
    {
        $attributes = $this->get_attributes();
        $args       = empty( $attributes['args'] ) ? [] : $attributes['args'];
        $required   = [];
        foreach ( $args as $key => $arg ) {
            if ( isset( $arg['required'] ) && true === $arg['required'] && null === $this->get_param( $key ) ) {
                $required[] = $key;
            }
        }
        if ( $required ) {
            return new \WP_Error(
                'rest_missing_callback_param',
                sprintf( __( 'Missing parameter(s): %s' ), implode( ', ', $required ) ),
                [ 'status' => 400, 'params' => $required ]
            );
        }
        $invalid_params  = [];
        $invalid_details = [];
        foreach ( $args as $key => $arg ) {
            $param = $this->get_param( $key );
            if ( null !== $param && ! empty( $arg['validate_callback'] ) ) {
                $valid_check = call_user_func( $arg['validate_callback'], $param, $this, $key );
                if ( false === $valid_check ) {
                    $invalid_params[ $key ] = __( 'Invalid parameter.' );
                }
                if ( is_wp_error( $valid_check ) ) {
                    $invalid_params[ $key ]  = implode( ' ', $valid_check->get_error_messages() );
                    $invalid_details[ $key ] = rest_convert_error_to_response( $valid_check )->get_data();
                }
            }
        }
        if ( $invalid_params ) {
            return new \WP_Error(
                'rest_invalid_param',
                sprintf( __( 'Invalid parameter(s): %s' ), implode( ', ', array_keys( $invalid_params ) ) ),
                [ 'status' => 400, 'params' => $invalid_params, 'details' => $invalid_details ]
            );
        }
        if ( isset( $attributes['validate_callback'] ) ) {
            $valid_check = call_user_func( $attributes['validate_callback'], $this );
            if ( is_wp_error( $valid_check ) ) {
                return $valid_check;
            }
            if ( false === $valid_check ) {
                return new \WP_Error( 'rest_invalid_params', __( 'Invalid parameters.' ), [ 'status' => 400 ] );
            }
        }
        return true;
    }

    /**
     * WordPress's own sanitization (WP_REST_Request::sanitize_params), reading through get_params() and writing
     * through set_param(): a typed argument without sanitize_callback gets rest_parse_request_arg, which validates.
     */
    public function sanitize_params()
    {
        $attributes = $this->get_attributes();
        if ( empty( $attributes['args'] ) ) {
            return true;
        }
        $invalid_params  = [];
        $invalid_details = [];
        foreach ( $this->get_params() as $key => $value ) {
            if ( ! isset( $attributes['args'][ $key ] ) ) {
                continue;
            }
            $param_args = $attributes['args'][ $key ];
            if ( ! array_key_exists( 'sanitize_callback', $param_args ) && ! empty( $param_args['type'] ) ) {
                $param_args['sanitize_callback'] = 'rest_parse_request_arg';
            }
            if ( empty( $param_args['sanitize_callback'] ) ) {
                continue;
            }
            $sanitized_value = call_user_func( $param_args['sanitize_callback'], $value, $this, $key );
            if ( is_wp_error( $sanitized_value ) ) {
                $invalid_params[ $key ]  = implode( ' ', $sanitized_value->get_error_messages() );
                $invalid_details[ $key ] = rest_convert_error_to_response( $sanitized_value )->get_data();
            } else {
                $this->set_param( $key, $sanitized_value );
            }
        }
        if ( $invalid_params ) {
            return new \WP_Error(
                'rest_invalid_param',
                sprintf( __( 'Invalid parameter(s): %s' ), implode( ', ', array_keys( $invalid_params ) ) ),
                [ 'status' => 400, 'params' => $invalid_params, 'details' => $invalid_details ]
            );
        }
        return true;
    }
    public function set_attributes( $attributes ) { }

    #[\ReturnTypeWillChange]
    public function offsetExists( $offset ) { return $this->has_param( $offset ); }

    #[\ReturnTypeWillChange]
    public function offsetUnset( $offset ) { $this->put( 'unset', (string) $offset ); }
}

/**
 * A `WP_REST_Response` holding the Rust REST response's data, status, headers and links. Hooks change it with any of
 * WordPress's methods or properties (`$response->data['x'] = …`, `add_link()`); the server reads it back afterwards.
 */
class ResponseProxy extends \WP_REST_Response
{
    public function __construct( int $handle )
    {
        parent::__construct(
            Native\response_get( $handle, 'data' ),
            Native\response_get( $handle, 'status' ),
            Native\response_get( $handle, 'headers' )
        );
        foreach ( Native\response_get( $handle, 'links' ) as $rel => $links ) {
            foreach ( $links as $link ) {
                $this->add_link( $rel, $link['href'], $link['attributes'] );
            }
        }
    }
}
