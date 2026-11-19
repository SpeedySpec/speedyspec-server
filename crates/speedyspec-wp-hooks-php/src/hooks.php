declare(strict_types=1);

namespace SpeedySpec\WpHooks {
    /**
     * Marks the proxies that stand in for WordPress's hook globals.
     */
    interface HookGlobal extends \ArrayAccess, \IteratorAggregate, \Countable
    {
    }

    /**
     * Stands in for `$wp_filter`: hook names to `WP_Hook` objects, kept in the Rust hook store.
     */
    final class FilterGlobal implements HookGlobal
    {
        public function offsetExists(mixed $offset): bool
        {
            return Native\global_has('filter', $offset);
        }

        public function offsetGet(mixed $offset): mixed
        {
            return Native\global_get('filter', $offset);
        }

        public function offsetSet(mixed $offset, mixed $value): void
        {
            Native\global_set('filter', $offset, $value);
        }

        public function offsetUnset(mixed $offset): void
        {
            Native\global_unset('filter', $offset);
        }

        public function getIterator(): \Iterator
        {
            return new \ArrayIterator(Native\global_all('filter'));
        }

        public function count(): int
        {
            return Native\global_count('filter');
        }
    }

    /**
     * Stands in for `$wp_actions` or `$wp_filters`: hook names to run counts, kept in the Rust hook store.
     */
    final class CountGlobal implements HookGlobal
    {
        public function __construct(private string $kind)
        {
        }

        public function offsetExists(mixed $offset): bool
        {
            return Native\global_has($this->kind, $offset);
        }

        public function offsetGet(mixed $offset): mixed
        {
            return Native\global_get($this->kind, $offset);
        }

        public function offsetSet(mixed $offset, mixed $value): void
        {
            Native\global_set($this->kind, $offset, $value);
        }

        public function offsetUnset(mixed $offset): void
        {
            Native\global_unset($this->kind, $offset);
        }

        public function getIterator(): \Iterator
        {
            return new \ArrayIterator(Native\global_all($this->kind));
        }

        public function count(): int
        {
            return Native\global_count($this->kind);
        }
    }

    /**
     * Stands in for `$wp_current_filter`: the names of the running hooks, kept in the Rust hook store.
     */
    final class CurrentFilterGlobal implements HookGlobal
    {
        public function offsetExists(mixed $offset): bool
        {
            return Native\global_has('current', $offset);
        }

        public function offsetGet(mixed $offset): mixed
        {
            return Native\global_get('current', $offset);
        }

        public function offsetSet(mixed $offset, mixed $value): void
        {
            Native\global_set('current', $offset, $value);
        }

        public function offsetUnset(mixed $offset): void
        {
            Native\global_unset('current', $offset);
        }

        public function getIterator(): \Iterator
        {
            return new \ArrayIterator(Native\global_all('current'));
        }

        public function count(): int
        {
            return Native\global_count('current');
        }
    }

    /**
     * Installs the hook globals, importing any hooks set before the request's PHP code ran.
     *
     * @internal
     */
    function install_globals(): void
    {
        $preinitialized = isset($GLOBALS['wp_filter']) && is_array($GLOBALS['wp_filter'])
            ? \WP_Hook::build_preinitialized_hooks($GLOBALS['wp_filter'])
            : [];
        $GLOBALS['wp_filter'] = new FilterGlobal();
        foreach ($preinitialized as $name => $hook) {
            $GLOBALS['wp_filter'][$name] = $hook;
        }
        $GLOBALS['wp_actions'] = new CountGlobal('actions');
        $GLOBALS['wp_filters'] = new CountGlobal('filters');
        $GLOBALS['wp_current_filter'] = new CurrentFilterGlobal();
    }
}

namespace {
    use SpeedySpec\WpHooks\Native;

    /**
     * Holds one hook's callbacks; its state lives in the Rust hook store, so this object is a handle.
     */
    #[\AllowDynamicProperties]
    final class WP_Hook implements Iterator, ArrayAccess, Countable
    {
        /**
         * The callbacks by priority; reads and writes go to the Rust hook.
         *
         * @var array<int|string, array<int|string, array>>
         */
        public $callbacks = array();

        private int $speedyspec_hook;

        private int $speedyspec_position = 0;

        public function __construct()
        {
            unset($this->callbacks);
            $this->speedyspec_hook = Native\hook_new($this);
        }

        /**
         * Wraps an existing Rust hook.
         *
         * @internal
         */
        public static function speedyspec_wrap(int $id): WP_Hook
        {
            $hook = (new ReflectionClass(self::class))->newInstanceWithoutConstructor();
            $bind = function (int $id): void {
                unset($this->callbacks);
                $this->speedyspec_hook = $id;
            };
            Closure::bind($bind, $hook, self::class)($id);
            return $hook;
        }

        public function __get(string $name): mixed
        {
            if ($name === 'callbacks') {
                return Native\hook_callbacks($this->speedyspec_hook);
            }
            trigger_error('Undefined property: WP_Hook::$' . $name, E_USER_WARNING);
            return null;
        }

        public function __set(string $name, mixed $value): void
        {
            if ($name === 'callbacks') {
                Native\hook_replace_callbacks($this->speedyspec_hook, $value);
                return;
            }
            $this->$name = $value;
        }

        public function __isset(string $name): bool
        {
            return $name === 'callbacks';
        }

        public function __unset(string $name): void
        {
            if ($name === 'callbacks') {
                Native\hook_replace_callbacks($this->speedyspec_hook, array());
            }
        }

        public function add_filter($hook_name, $callback, $priority, $accepted_args)
        {
            Native\hook_add_filter($this->speedyspec_hook, $hook_name, $callback, $priority, (int) $accepted_args);
        }

        public function remove_filter($hook_name, $callback, $priority)
        {
            return Native\hook_remove_filter($this->speedyspec_hook, $hook_name, $callback, $priority);
        }

        public function has_filter($hook_name = '', $callback = false, $priority = false)
        {
            return Native\hook_has_filter($this->speedyspec_hook, $hook_name, $callback, $priority);
        }

        public function has_filters()
        {
            return Native\hook_has_filters($this->speedyspec_hook);
        }

        public function remove_all_filters($priority = false)
        {
            Native\hook_remove_all_filters($this->speedyspec_hook, $priority);
        }

        public function apply_filters($value, $args)
        {
            return Native\hook_apply_filters($this->speedyspec_hook, $value, $args);
        }

        public function do_action($args)
        {
            Native\hook_do_action($this->speedyspec_hook, $args);
        }

        public function do_all_hook(&$args)
        {
            Native\hook_do_all_hook($this->speedyspec_hook, $args);
        }

        public function current_priority()
        {
            return Native\hook_current_priority($this->speedyspec_hook);
        }

        public static function build_preinitialized_hooks($filters)
        {
            /** @var WP_Hook[] $normalized */
            $normalized = array();

            foreach ($filters as $hook_name => $callback_groups) {
                if ($callback_groups instanceof WP_Hook) {
                    $normalized[$hook_name] = $callback_groups;
                    continue;
                }

                $hook = new WP_Hook();

                // Loop through callback groups.
                foreach ($callback_groups as $priority => $callbacks) {
                    // Loop through callbacks.
                    foreach ($callbacks as $cb) {
                        $hook->add_filter($hook_name, $cb['function'], $priority, $cb['accepted_args']);
                    }
                }

                $normalized[$hook_name] = $hook;
            }

            return $normalized;
        }

        #[ReturnTypeWillChange]
        public function offsetExists($offset)
        {
            return Native\hook_offset_exists($this->speedyspec_hook, $offset);
        }

        #[ReturnTypeWillChange]
        public function offsetGet($offset)
        {
            return Native\hook_offset_get($this->speedyspec_hook, $offset);
        }

        #[ReturnTypeWillChange]
        public function offsetSet($offset, $value)
        {
            Native\hook_offset_set($this->speedyspec_hook, $offset, $value);
        }

        #[ReturnTypeWillChange]
        public function offsetUnset($offset)
        {
            Native\hook_offset_unset($this->speedyspec_hook, $offset);
        }

        #[ReturnTypeWillChange]
        public function current()
        {
            $keys = Native\hook_priorities($this->speedyspec_hook);
            if (!array_key_exists($this->speedyspec_position, $keys)) {
                return false;
            }
            return Native\hook_offset_get($this->speedyspec_hook, $keys[$this->speedyspec_position]);
        }

        #[ReturnTypeWillChange]
        public function next()
        {
            ++$this->speedyspec_position;
            return $this->current();
        }

        #[ReturnTypeWillChange]
        public function key()
        {
            $keys = Native\hook_priorities($this->speedyspec_hook);
            return $keys[$this->speedyspec_position] ?? null;
        }

        #[ReturnTypeWillChange]
        public function valid()
        {
            return $this->key() !== null;
        }

        #[ReturnTypeWillChange]
        public function rewind()
        {
            $this->speedyspec_position = 0;
        }

        #[ReturnTypeWillChange]
        public function count()
        {
            return count(Native\hook_priorities($this->speedyspec_hook));
        }
    }

    function add_filter($hook_name, $callback, $priority = 10, $accepted_args = 1)
    {
        return Native\add_filter($hook_name, $callback, $priority, (int) $accepted_args);
    }

    function apply_filters($hook_name, $value, ...$args)
    {
        return Native\apply_filters($hook_name, $value, ...$args);
    }

    function apply_filters_ref_array($hook_name, $args)
    {
        return Native\apply_filters_ref_array($hook_name, $args);
    }

    function has_filter($hook_name, $callback = false, $priority = false)
    {
        return Native\has_filter($hook_name, $callback, $priority);
    }

    function remove_filter($hook_name, $callback, $priority = 10)
    {
        return Native\remove_filter($hook_name, $callback, $priority);
    }

    function remove_all_filters($hook_name, $priority = false)
    {
        return Native\remove_all_filters($hook_name, $priority);
    }

    function current_filter()
    {
        return Native\current_filter();
    }

    function doing_filter($hook_name = null)
    {
        return Native\doing_filter($hook_name);
    }

    function did_filter($hook_name)
    {
        return Native\did_filter($hook_name);
    }

    function add_action($hook_name, $callback, $priority = 10, $accepted_args = 1)
    {
        return Native\add_filter($hook_name, $callback, $priority, (int) $accepted_args);
    }

    function do_action($hook_name, ...$arg)
    {
        Native\do_action($hook_name, ...$arg);
    }

    function do_action_ref_array($hook_name, $args)
    {
        Native\do_action_ref_array($hook_name, $args);
    }

    function has_action($hook_name, $callback = false, $priority = false)
    {
        return Native\has_filter($hook_name, $callback, $priority);
    }

    function remove_action($hook_name, $callback, $priority = 10)
    {
        return Native\remove_filter($hook_name, $callback, $priority);
    }

    function remove_all_actions($hook_name, $priority = false)
    {
        return Native\remove_all_filters($hook_name, $priority);
    }

    function current_action()
    {
        return Native\current_filter();
    }

    function doing_action($hook_name = null)
    {
        return Native\doing_filter($hook_name);
    }

    function did_action($hook_name)
    {
        return Native\did_action($hook_name);
    }

    function apply_filters_deprecated($hook_name, $args, $version, $replacement = '', $message = '')
    {
        return Native\apply_filters_deprecated($hook_name, $args, $version, $replacement, $message);
    }

    function do_action_deprecated($hook_name, $args, $version, $replacement = '', $message = '')
    {
        Native\do_action_deprecated($hook_name, $args, $version, $replacement, $message);
    }

    function _wp_call_all_hook($args)
    {
        Native\call_all_hook($args);
    }

    function _wp_filter_build_unique_id($hook_name, $callback, $priority): ?string
    {
        return Native\unique_id($callback);
    }
}
