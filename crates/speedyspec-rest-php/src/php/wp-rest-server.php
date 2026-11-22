declare(strict_types=1);

/**
 * WordPress's REST server, as a shim over the Rust REST server: routes registered here become operations served by
 * Rust, and dispatch goes through Rust. WordPress's own helpers (response_to_data, link compaction, envelopes, the
 * index shapes) come from SpeedySpec_TrunkRestServer, a renamed copy of WordPress's class.
 */
class WP_REST_Server extends SpeedySpec_TrunkRestServer
{
    public function register_route( $route_namespace, $route, $route_args, $override = false )
    {
        parent::register_route( $route_namespace, $route, $route_args, $override );

        $defaults = [
            'methods'       => '',
            'accept_json'   => false,
            'accept_raw'    => false,
            'show_in_index' => true,
            'args'          => [],
        ];
        unset( $route_args['namespace'] );
        if ( isset( $route_args['callback'] ) ) {
            $route_args = [ $route_args ];
        }
        $handlers = [];
        foreach ( $route_args as $key => $handler ) {
            if ( ! is_numeric( $key ) || ! is_array( $handler ) ) {
                continue;
            }
            $handler = array_merge( $defaults, $handler );
            if ( is_string( $handler['methods'] ) ) {
                $methods = explode( ',', $handler['methods'] );
            } elseif ( is_array( $handler['methods'] ) ) {
                $methods = $handler['methods'];
            } else {
                $methods = [];
            }
            $handler['methods'] = array_values( array_unique( array_map( static fn ( $m ): string => strtoupper( trim( (string) $m ) ), $methods ) ) );
            $handlers[] = $handler;
        }
        \SpeedySpec\Rest\Native\legacy_register( (string) $route_namespace, (string) $route, $handlers, (bool) $override );
    }
}
