declare(strict_types=1);

namespace SpeedySpec\Cqrs;

/**
 * Thrown when a bus cannot handle a message.
 */
class BusException extends \RuntimeException
{
}

/**
 * Dispatches commands, by their handler's key, through the Rust command bus and its middleware.
 */
final class CommandBus
{
    public static function dispatch(string $key, mixed $command = null): mixed
    {
        return Native\dispatch($key, $command);
    }
}

/**
 * Asks queries, by their handler's key, through the Rust query bus and its middleware.
 */
final class QueryBus
{
    public static function ask(string $key, mixed $query = null): mixed
    {
        return Native\ask($key, $query);
    }
}
