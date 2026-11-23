declare(strict_types=1);

namespace SpeedySpec\XmlRpc;

/**
 * Binds operations to XML-RPC methods.
 */
final class XmlRpc
{
    /**
     * Starts the XML-RPC binding of an operation.
     *
     * `XmlRpc::bind( 'acme/update-report' )->method( 'acme.updateReport' )` names the method and keeps the default
     * parameters; `->params( 'blog_id', 'username', 'password', 'id', '*' )` names them too.
     *
     * @param string $operation
     *   The operation's name.
     * @return XmlRpcBinder
     *   The binder that stores the binding.
     */
    public static function bind(string $operation): XmlRpcBinder
    {
        return new XmlRpcBinder($operation);
    }
}

/**
 * Stores one operation's XML-RPC binding.
 */
final class XmlRpcBinder
{
    private string $method = '';

    public function __construct(private readonly string $operation)
    {
    }

    /**
     * Names the method and stores the binding with the default parameters.
     *
     * @param string $name
     *   A dotted method name outside `system.`, such as `acme.updateReport`.
     * @return self
     *   This binder, so parameters can follow.
     */
    public function method(string $name): self
    {
        $this->method = $name;
        Native\bind($this->operation, $name, null);
        return $this;
    }

    /**
     * Names the positional parameters and stores the binding.
     *
     * Each name is `blog_id`, `username`, `password`, an input property, or `*` for one struct of the properties
     * left; the transport checks the properties when the operations are exposed.
     *
     * @param string ...$names
     *   The parameters, in order.
     */
    public function params(string ...$names): void
    {
        foreach ($names as $name) {
            if ('*' !== $name && 1 !== preg_match('/^[A-Za-z_][A-Za-z0-9_]*$/', $name)) {
                throw new \InvalidArgumentException(sprintf('"%s" is not an XML-RPC parameter name.', $name));
            }
        }
        Native\bind($this->operation, $this->method, $names);
    }
}

/**
 * An operation in WordPress's XML-RPC method table.
 *
 * `xmlrpc_methods` callbacks see operations as these callable entries; calling one runs the operation as its method
 * would run.
 */
final class OperationMethod
{
    public function __construct(public readonly string $operation)
    {
    }

    /**
     * Runs the operation with positional parameters.
     *
     * @param mixed $args
     *   The parameters, a list, or one parameter as IXR passes a single one.
     * @return mixed
     *   The result, or an IXR_Error.
     */
    public function __invoke(mixed $args = []): mixed
    {
        return Internal\Adapter::result(Native\call_operation($this->operation, Internal\Adapter::tag_params($args)));
    }
}

namespace SpeedySpec\XmlRpc\Internal;

/**
 * Between WordPress's XML-RPC objects and the Rust XML-RPC transport.
 *
 * @internal
 */
final class Adapter
{
    /**
     * Decides whether XML-RPC methods that log in are enabled, as `wp_xmlrpc_server::set_is_enabled()` does.
     *
     * @return bool
     *   The filtered answer.
     */
    public static function enabled(): bool
    {
        $is_enabled = apply_filters('pre_option_enable_xmlrpc', false, 'enable_xmlrpc', false);
        if (false === $is_enabled) {
            $is_enabled = apply_filters('option_enable_xmlrpc', true, 'enable_xmlrpc');
        }
        return (bool) apply_filters('xmlrpc_enabled', $is_enabled);
    }

    /**
     * Filters the element limit of one request.
     *
     * @param int $default
     *   WordPress's default.
     * @return int
     *   The filtered limit; anything but an integer keeps the default.
     */
    public static function element_limit(int $default): int
    {
        $limit = apply_filters('xmlrpc_element_limit', $default);
        return is_int($limit) ? $limit : $default;
    }

    /**
     * Fires `xmlrpc_call` for an operation method.
     *
     * @param string $method
     *   The method name.
     * @param array $args
     *   The tagged parameters.
     */
    public static function call(string $method, array $args): void
    {
        do_action('xmlrpc_call', $method, self::untag(['t' => 'l', 'v' => $args]), self::server());
    }

    /**
     * Filters a failed login's fault through `xmlrpc_login_error`.
     *
     * @param int $code
     *   The fault code.
     * @param string $message
     *   The fault message.
     * @return array
     *   The filtered `[ code, message ]`.
     */
    public static function login_error(int $code, string $message): array
    {
        if (! class_exists('IXR_Error', false)) {
            return [$code, $message];
        }
        $user  = class_exists('WP_Error', false) ? new \WP_Error('invalid_login', $message) : null;
        $error = apply_filters('xmlrpc_login_error', new \IXR_Error($code, $message), $user);
        if ($error instanceof \IXR_Error) {
            return [(int) $error->code, htmlspecialchars_decode((string) $error->message, ENT_QUOTES)];
        }
        return [$code, $message];
    }

    /**
     * Logs in with WordPress's `wp_authenticate()` and makes the user current.
     *
     * @param string $username
     *   The username sent in the call.
     * @param string $password
     *   The password sent in the call.
     * @return int|null
     *   The user's ID, or null when the credentials are refused.
     */
    public static function login(string $username, #[\SensitiveParameter] string $password): ?int
    {
        $user = wp_authenticate($username, $password);
        if (! is_object($user) || is_wp_error($user)) {
            return null;
        }
        wp_set_current_user($user->ID);
        return (int) $user->ID;
    }

    /**
     * Holds the request's `wp_xmlrpc_server`, which WordPress passes to `xmlrpc_call`.
     *
     * @return object|null
     *   The server, or null before one exists.
     */
    public static function server(): ?object
    {
        return $GLOBALS['speedyspec_xmlrpc_server'] ?? null;
    }

    /**
     * Adds the exposed operations to WordPress's method table, keeping the methods already there.
     *
     * @param array $methods
     *   The method table.
     * @return array
     *   The table with the operations.
     */
    public static function add_operations($methods)
    {
        $methods = is_array($methods) ? $methods : [];
        foreach (\SpeedySpec\XmlRpc\Native\operation_methods() as $method => $operation) {
            if (! array_key_exists($method, $methods)) {
                $methods[$method] = new \SpeedySpec\XmlRpc\OperationMethod($operation);
            }
        }
        return $methods;
    }

    /**
     * Creates the request's server through `wp_xmlrpc_server_class` when none exists, as `xmlrpc.php` does.
     */
    public static function ensure_server(): void
    {
        if (null === self::server()) {
            $class = apply_filters('wp_xmlrpc_server_class', 'wp_xmlrpc_server');
            new $class();
        }
    }

    /**
     * Lists the request's method table for the Rust server.
     *
     * @return array
     *   `[ method, 'op', operation ]` for an operation, `[ method, 'legacy', callback ]` for anything else.
     */
    public static function table(): array
    {
        $rows = [];
        foreach ((array) (self::server()->methods ?? []) as $method => $callback) {
            $rows[] = $callback instanceof \SpeedySpec\XmlRpc\OperationMethod
                ? [(string) $method, 'op', $callback->operation]
                : [(string) $method, 'legacy', $callback];
        }
        return $rows;
    }

    /**
     * Runs one legacy method as IXR_Server::call() runs it: a single parameter unwrapped, `this:` methods on the
     * request's server, IXR's faults for a missing method.
     *
     * @param mixed $callback
     *   The table entry.
     * @param array $params
     *   The tagged parameters.
     * @return array
     *   The tagged result, or a tagged fault.
     */
    public static function invoke(mixed $callback, array $params): array
    {
        $args = self::untag(['t' => 'l', 'v' => $params]);
        if (1 === count($args)) {
            $args = $args[0];
        }
        $server = self::server();
        if (is_string($callback) && str_starts_with($callback, 'this:')) {
            $method = substr($callback, 5);
            if (null === $server || ! method_exists($server, $method)) {
                return self::fault(-32601, 'server error. requested class method "' . $method . '" does not exist.');
            }
            return self::tag_result($server->$method($args));
        }
        if (is_array($callback)) {
            if (! is_callable([$callback[0] ?? null, $callback[1] ?? null])) {
                return self::fault(-32601, 'server error. requested object method "' . ($callback[1] ?? '') . '" does not exist.');
            }
        } elseif (is_string($callback) && ! function_exists($callback)) {
            return self::fault(-32601, 'server error. requested function "' . $callback . '" does not exist.');
        } elseif (! is_callable($callback)) {
            return self::fault(-32601, 'server error. requested function does not exist.');
        }
        return self::tag_result(call_user_func($callback, $args));
    }

    /**
     * Describes a method's result: a tagged fault for an IXR_Error, the tagged value otherwise.
     *
     * @param mixed $result
     *   The result.
     * @return array
     *   The tagged result.
     */
    private static function tag_result(mixed $result): array
    {
        if (class_exists('IXR_Error', false) && $result instanceof \IXR_Error) {
            // IXR_Error escapes its message; the Rust side holds the text, and escapes it once when it is sent.
            return self::fault((int) $result->code, htmlspecialchars_decode((string) $result->message, ENT_QUOTES));
        }
        return self::tag($result);
    }

    /**
     * Describes a fault.
     *
     * @param int $code
     *   The fault code.
     * @param string $message
     *   The fault message.
     * @return array
     *   The tagged fault.
     */
    private static function fault(int $code, string $message): array
    {
        return ['t' => 'fault', 'code' => $code, 'message' => $message];
    }

    /**
     * Builds the PHP value of a tagged result: an IXR_Error for a fault.
     *
     * @param array $tagged
     *   The tagged result.
     * @return mixed
     *   The value, or an IXR_Error.
     */
    public static function result(array $tagged): mixed
    {
        if ('fault' === ($tagged['t'] ?? null)) {
            return new \IXR_Error((int) $tagged['code'], (string) $tagged['message']);
        }
        return self::untag($tagged);
    }

    /**
     * Describes positional parameters: a list as it is, any other value as the one parameter.
     *
     * @param mixed $args
     *   The parameters.
     * @return array
     *   The tagged parameters.
     */
    public static function tag_params(mixed $args): array
    {
        $args = is_array($args) && array_is_list($args) ? $args : [$args];
        return array_map([self::class, 'tag'], $args);
    }

    /**
     * Declares the wp_xmlrpc_server shim when WordPress's code first names it.
     *
     * @param string $class
     *   The class being loaded.
     */
    public static function autoload(string $class): void
    {
        if (! in_array(strtolower($class), ['wp_xmlrpc_server', 'speedyspec_trunkxmlrpcserver'], true)) {
            return;
        }
        if (! class_exists('IXR_Server', false) && defined('ABSPATH') && defined('WPINC')) {
            require_once ABSPATH . WPINC . '/class-IXR.php';
        }
        \SpeedySpec\XmlRpc\Native\declare_shim();
    }

    /**
     * Describes a PHP value as IXR would encode it, as tagged arrays the Rust side reads.
     *
     * @param mixed $value
     *   The value.
     * @return array
     *   `[ 't' => kind, 'v' => value ]`: `n` null, `b` bool, `i` int, `f` float, `s` string, `d` IXR date (ISO),
     *   `x` base64 (the bytes base64-encoded), `l` list, `m` map (a list of `[ key, tagged ]` pairs).
     */
    public static function tag(mixed $value): array
    {
        if (null === $value) {
            return ['t' => 'n'];
        }
        if (is_bool($value)) {
            return ['t' => 'b', 'v' => $value];
        }
        if (is_int($value)) {
            return ['t' => 'i', 'v' => $value];
        }
        if (is_float($value)) {
            return ['t' => 'f', 'v' => $value];
        }
        if (class_exists('IXR_Date', false) && $value instanceof \IXR_Date) {
            return ['t' => 'd', 'v' => $value->getIso()];
        }
        if (class_exists('IXR_Base64', false) && $value instanceof \IXR_Base64) {
            return ['t' => 'x', 'v' => base64_encode((string) $value->data)];
        }
        if (is_object($value)) {
            return self::tag_map(get_object_vars($value));
        }
        if (is_array($value)) {
            $expected = 0;
            foreach ($value as $key => $item) {
                if ((string) $key !== (string) $expected) {
                    return self::tag_map($value);
                }
                ++$expected;
            }
            return ['t' => 'l', 'v' => array_map([self::class, 'tag'], $value)];
        }
        return ['t' => 's', 'v' => (string) $value];
    }

    /**
     * Describes an array as a tagged struct.
     *
     * @param array $value
     *   The array.
     * @return array
     *   The tagged map.
     */
    private static function tag_map(array $value): array
    {
        $pairs = [];
        foreach ($value as $key => $item) {
            $pairs[] = [(string) $key, self::tag($item)];
        }
        return ['t' => 'm', 'v' => $pairs];
    }

    /**
     * Builds the PHP value a tagged array describes, with IXR's date and base64 objects.
     *
     * @param array $tagged
     *   The tagged value.
     * @return mixed
     *   The PHP value.
     */
    public static function untag(array $tagged): mixed
    {
        $value = $tagged['v'] ?? null;
        switch ($tagged['t'] ?? 'n') {
            case 'd':
                return class_exists('IXR_Date', false) ? new \IXR_Date((string) $value) : (string) $value;
            case 'x':
                $bytes = (string) base64_decode((string) $value, true);
                return class_exists('IXR_Base64', false) ? new \IXR_Base64($bytes) : $bytes;
            case 'l':
                return array_map([self::class, 'untag'], (array) $value);
            case 'm':
                $map = [];
                foreach ((array) $value as [$key, $item]) {
                    $map[$key] = self::untag($item);
                }
                return $map;
            case 'n':
                return null;
            default:
                return $value;
        }
    }
}

spl_autoload_register([Adapter::class, 'autoload']);
