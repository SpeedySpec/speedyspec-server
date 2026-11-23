declare(strict_types=1);

use SpeedySpec\XmlRpc\Internal\Adapter;
use SpeedySpec\XmlRpc\Native;

/**
 * WordPress's XML-RPC server, as a shim over the Rust XML-RPC server.
 *
 * The method table WordPress and plugins build (`xmlrpc_methods`) is served by Rust: each entry runs as an operation,
 * and new-style operations join the table as {@link SpeedySpec\XmlRpc\OperationMethod} entries. WordPress's method
 * bodies come from SpeedySpec_TrunkXmlRpcServer, a renamed copy of WordPress's class. One instance is one request's
 * server: constructing another starts over, as WordPress's `auth_failed` does.
 */
class wp_xmlrpc_server extends SpeedySpec_TrunkXmlRpcServer
{
    /**
     * Builds WordPress's method table, with the exposed operations added before plugins filter it.
     */
    public function __construct()
    {
        Native\reset();
        $add = [Adapter::class, 'add_operations'];
        if (false === has_filter('xmlrpc_methods', $add)) {
            add_filter('xmlrpc_methods', $add, PHP_INT_MIN);
        }
        parent::__construct();
        $GLOBALS['speedyspec_xmlrpc_server'] = $this;
    }

    /**
     * Logs in through the Rust server, so operation and legacy methods share one failed-login latch.
     *
     * @param string $username
     *   The username sent in the call.
     * @param string $password
     *   The password sent in the call.
     * @return WP_User|false
     *   The user, or false with the fault in `$this->error`.
     */
    public function login($username, #[\SensitiveParameter] $password)
    {
        $result = Native\login((string) $username, (string) $password);
        if (is_int($result)) {
            return function_exists('get_userdata') ? get_userdata($result) : false;
        }
        $this->error = new IXR_Error((int) $result[0], (string) $result[1]);
        return false;
    }

    /**
     * Calls any method, WordPress's, a plugin's or an operation's, through the Rust server.
     *
     * @param string $methodname
     *   The method name.
     * @param mixed $args
     *   The positional parameters.
     * @return mixed
     *   The method's result, or an IXR_Error.
     */
    public function call($methodname, $args)
    {
        return Adapter::result(Native\call((string) $methodname, Adapter::tag_params($args)));
    }

    /**
     * Serves this request's XML-RPC call through the Rust server.
     */
    public function serve_request()
    {
        [$status, $headers, $body] = Native\serve(
            (string) ($_SERVER['REQUEST_METHOD'] ?? 'GET'),
            (string) file_get_contents('php://input')
        );
        http_response_code((int) $status);
        foreach ($headers as $name => $value) {
            header($name . ': ' . $value);
        }
        echo $body;
    }
}
