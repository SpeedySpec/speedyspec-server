declare(strict_types=1);

namespace SpeedySpec\Container;

/**
 * Thrown when the container cannot resolve, bind or alias a key.
 */
class ContainerException extends \RuntimeException
{
}

/**
 * The service container, from PHP. Every method is backed by the Rust container (`SpeedySpec\Container\Native\*`);
 * this class holds no state of its own. Everything PHP registers lasts until the end of the request.
 */
final class Container
{
    private static ?Container $instance = null;

    private function __construct()
    {
    }

    public static function getInstance(): Container
    {
        return self::$instance ??= new self();
    }

    public function bind(string $abstract, \Closure|string|null $concrete = null, bool $shared = false): void
    {
        Native\bind($abstract, $concrete ?? $abstract, $shared ? 1 : 0);
    }

    public function bindIf(string $abstract, \Closure|string|null $concrete = null, bool $shared = false): void
    {
        if (!$this->bound($abstract)) {
            $this->bind($abstract, $concrete, $shared);
        }
    }

    public function singleton(string $abstract, \Closure|string|null $concrete = null): void
    {
        Native\bind($abstract, $concrete ?? $abstract, 1);
    }

    public function singletonIf(string $abstract, \Closure|string|null $concrete = null): void
    {
        if (!$this->bound($abstract)) {
            $this->singleton($abstract, $concrete);
        }
    }

    public function scoped(string $abstract, \Closure|string|null $concrete = null): void
    {
        Native\bind($abstract, $concrete ?? $abstract, 2);
    }

    public function scopedIf(string $abstract, \Closure|string|null $concrete = null): void
    {
        if (!$this->bound($abstract)) {
            $this->scoped($abstract, $concrete);
        }
    }

    public function instance(string $abstract, mixed $instance): mixed
    {
        Native\instance($abstract, $instance);
        return $instance;
    }

    public function make(string $abstract, array $parameters = []): mixed
    {
        return Native\make($abstract, $parameters);
    }

    public function makeWith(string $abstract, array $parameters = []): mixed
    {
        return Native\make($abstract, $parameters);
    }

    public function get(string $id): mixed
    {
        return Native\make($id, []);
    }

    public function has(string $id): bool
    {
        return $this->bound($id);
    }

    public function bound(string $abstract): bool
    {
        return Native\has($abstract);
    }

    public function resolved(string $abstract): bool
    {
        return Native\resolved($abstract);
    }

    public function alias(string $abstract, string $alias): void
    {
        Native\alias($abstract, $alias);
    }

    public function isAlias(string $name): bool
    {
        return Native\is_alias($name);
    }

    public function getAlias(string $abstract): string
    {
        return Native\get_alias($abstract);
    }

    public function extend(string $abstract, \Closure $closure): void
    {
        Native\extend($abstract, $closure);
    }

    public function when(array|string $concrete): ContextualBindingBuilder
    {
        return new ContextualBindingBuilder((array) $concrete);
    }

    public function tag(array|string $abstracts, array|string ...$tags): void
    {
        foreach ($tags as $tag) {
            foreach ((array) $tag as $name) {
                Native\tag(array_values((array) $abstracts), $name);
            }
        }
    }

    public function tagged(string $tag): array
    {
        return Native\tagged($tag);
    }

    public function resolving(\Closure|string $abstract, ?\Closure $callback = null): void
    {
        $abstract instanceof \Closure ? Native\on(0, null, $abstract) : Native\on(0, $abstract, $callback);
    }

    public function afterResolving(\Closure|string $abstract, ?\Closure $callback = null): void
    {
        $abstract instanceof \Closure ? Native\on(1, null, $abstract) : Native\on(1, $abstract, $callback);
    }

    public function beforeResolving(\Closure|string $abstract, ?\Closure $callback = null): void
    {
        $abstract instanceof \Closure ? Native\on(2, null, $abstract) : Native\on(2, $abstract, $callback);
    }

    /**
     * Calls `$callback`, resolving each parameter that `$parameters` does not name from the container.
     * `Class@method` makes the class first.
     */
    public function call(callable|string $callback, array $parameters = []): mixed
    {
        if (is_string($callback) && str_contains($callback, '@')) {
            [$class, $method] = explode('@', $callback, 2);
            $callback = [$this->make($class), $method];
        }
        if (is_array($callback)) {
            $reflector = new \ReflectionMethod($callback[0], $callback[1]);
        } elseif (is_string($callback) && str_contains($callback, '::')) {
            $reflector = new \ReflectionMethod($callback);
        } else {
            $reflector = new \ReflectionFunction(\Closure::fromCallable($callback));
        }
        return $callback(...resolve_parameters($reflector->getParameters(), $parameters));
    }

    public function factory(string $abstract): \Closure
    {
        return fn () => $this->make($abstract);
    }

    public function forgetInstance(string $abstract): void
    {
        Native\forget(0, $abstract);
    }

    public function forgetInstances(): void
    {
        Native\forget(1, null);
    }

    public function forgetScopedInstances(): void
    {
        Native\forget(2, null);
    }

    /**
     * Forgets everything PHP registered in this request; Rust's own bindings stay.
     */
    public function flush(): void
    {
        Native\forget(3, null);
    }
}

final class ContextualBindingBuilder
{
    private string $needs = '';

    /**
     * @param string[] $concrete
     */
    public function __construct(private array $concrete)
    {
    }

    public function needs(string $abstract): self
    {
        $this->needs = $abstract;
        return $this;
    }

    public function give(\Closure|string $implementation): void
    {
        foreach ($this->concrete as $concrete) {
            Native\give($concrete, $this->needs, $implementation);
        }
    }
}

/**
 * Builds `$class`, resolving its constructor's parameters (the container's autowiring).
 *
 * @internal
 */
function autowire(string $class, array $parameters): object
{
    $reflector = new \ReflectionClass($class);
    if (!$reflector->isInstantiable()) {
        throw new ContainerException("$class is not instantiable");
    }
    $constructor = $reflector->getConstructor();
    if ($constructor === null) {
        return new $class();
    }
    return $reflector->newInstanceArgs(resolve_parameters($constructor->getParameters(), $parameters));
}

/**
 * @internal
 * @param \ReflectionParameter[] $parameters
 */
function resolve_parameters(array $parameters, array $given): array
{
    $container = Container::getInstance();
    $arguments = [];
    foreach ($parameters as $parameter) {
        $name = $parameter->getName();
        if (array_key_exists($name, $given)) {
            $arguments[] = $given[$name];
            continue;
        }
        if (array_key_exists($parameter->getPosition(), $given)) {
            $arguments[] = $given[$parameter->getPosition()];
            continue;
        }
        $type = $parameter->getType();
        if ($type instanceof \ReflectionNamedType && !$type->isBuiltin()) {
            try {
                $arguments[] = $container->make($type->getName());
                continue;
            } catch (ContainerException $e) {
                if (!$parameter->isDefaultValueAvailable() && !$parameter->allowsNull()) {
                    throw $e;
                }
            }
        }
        if ($parameter->isDefaultValueAvailable()) {
            $arguments[] = $parameter->getDefaultValue();
            continue;
        }
        if ($parameter->isVariadic()) {
            break;
        }
        if ($parameter->allowsNull()) {
            $arguments[] = null;
            continue;
        }
        $owner = $parameter->getDeclaringClass()?->getName() ?? $parameter->getDeclaringFunction()->getName();
        throw new ContainerException("unresolvable parameter \$$name of $owner");
    }
    return $arguments;
}
